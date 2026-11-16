// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The programs of a terminal on Windows, from the list of processes. Windows tells neither
//! the working directory nor the command line of another process without reading its
//! memory, so the directory comes from the marks of the shell and the command line is the
//! name of the program alone.

use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess,
};

use super::tree::{self, Process};
use super::{Programs, Signal};

/// The shell of a `ConPTY` and what it runs.
pub struct Console {
    shell: u32,
    /// When the shell started; `None` once it ended.
    shell_start: Option<u64>,
}

impl Console {
    /// The console whose shell has the process id `shell`.
    #[must_use]
    pub fn new(shell: u32) -> Self {
        Self {
            shell,
            shell_start: start_time(shell),
        }
    }

    fn foreground(&self) -> (Vec<Process>, u32) {
        let processes = processes();
        let started = |id: u32| {
            if id == self.shell {
                self.shell_start
            } else {
                start_time(id)
            }
        };
        // A process counts as a child only if it started after its parent; one that cannot
        // be opened does not count, so that nothing unchecked is ever ended.
        let counts = |parent: u32, child: &Process| {
            started(parent)
                .zip(start_time(child.id))
                .is_some_and(|(parent, child)| child >= parent)
        };
        let foreground = tree::foreground(&processes, self.shell, counts);
        (processes, foreground)
    }
}

/// When the process of `handle` started, in the ticks of Windows.
fn creation_time(handle: HANDLE) -> Option<u64> {
    // SAFETY: the times are valid for the call, and a stale handle only makes it fail.
    unsafe {
        let mut times = [std::mem::zeroed::<FILETIME>(); 4];
        let [created, exited, kernel, user] = &mut times;
        (GetProcessTimes(handle, created, exited, kernel, user) != 0)
            .then(|| u64::from(times[0].dwHighDateTime) << 32 | u64::from(times[0].dwLowDateTime))
    }
}

/// When process `id` started; `None` if it cannot be opened.
fn start_time(id: u32) -> Option<u64> {
    // SAFETY: plain system calls on a handle that is checked and closed again.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id);
        if process.is_null() {
            return None;
        }
        let started = creation_time(process);
        CloseHandle(process);
        started
    }
}

/// The name of the program behind process `pid`; for `kudari mcp`, the binary `own`, the
/// agent which started it.
pub(super) fn program(pid: u32, own: Option<&Path>) -> Option<String> {
    let is_own = |id| own.is_some_and(|own| image(id).is_some_and(|image| same_file(&image, own)));
    tree::program(&processes(), pid, is_own)
}

/// Whether two paths name the same file, as Windows ignores case.
fn same_file(one: &Path, other: &Path) -> bool {
    match (std::fs::canonicalize(one), std::fs::canonicalize(other)) {
        (Ok(one), Ok(other)) => one.as_os_str().eq_ignore_ascii_case(other.as_os_str()),
        _ => false,
    }
}

/// The path of the program of process `pid`.
fn image(pid: u32) -> Option<PathBuf> {
    let mut path = vec![0u16; 32_768];
    let mut length = path.len() as u32;
    // SAFETY: plain system calls on a handle that is checked and closed again; the buffer
    // holds `length` units.
    let found = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let found = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &raw mut length);
        CloseHandle(process);
        found != 0
    };
    let path = path.get(..length as usize).filter(|_| found)?;
    Some(PathBuf::from(std::ffi::OsString::from_wide(path)))
}

/// All processes, in the order Windows lists them, which is roughly the order they started.
fn processes() -> Vec<Process> {
    let mut found = Vec::new();
    // SAFETY: plain system calls; the entry is valid for each and the snapshot is closed at
    // the end.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return found;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &raw mut entry) != 0;
        while more {
            let length = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            found.push(Process {
                id: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(entry.szExeFile.get(..length).unwrap_or_default()),
            });
            more = Process32NextW(snapshot, &raw mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    found
}

impl Programs for Console {
    fn is_busy(&self) -> bool {
        self.foreground().1 != self.shell
    }

    fn working_directory(&self) -> Option<PathBuf> {
        None
    }

    fn name(&self) -> Option<String> {
        let (processes, foreground) = self.foreground();
        tree::name(&processes, foreground)
    }

    fn arguments(&self) -> Option<Vec<String>> {
        let (processes, foreground) = self.foreground();
        let process = processes.iter().find(|process| process.id == foreground)?;
        Some(vec![process.name.clone()])
    }

    /// Interrupting is Ctrl+C, which the terminal types; terminating and killing end the
    /// program in the foreground.
    fn signal(&self, signal: Signal) {
        if !matches!(signal, Signal::Terminate | Signal::Kill) {
            return;
        }
        let (_, foreground) = self.foreground();
        // SAFETY: plain system calls on a handle that is checked and closed again.
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, foreground);
            if !process.is_null() {
                // Its id can have gone to another process since the list: only one that
                // started with the shell or after it ends.
                let ours = creation_time(process)
                    .zip(self.shell_start)
                    .is_some_and(|(started, shell)| started >= shell);
                if ours {
                    TerminateProcess(process, 1);
                }
                CloseHandle(process);
            }
        }
    }

    fn supports(&self, signal: Signal) -> bool {
        matches!(signal, Signal::Interrupt | Signal::Terminate | Signal::Kill)
    }

    /// The console has no flow control to switch.
    fn set_flow_control(&self, _enabled: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_the_shell_started_is_in_the_foreground() {
        let mut shell = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 6 127.0.0.1 > nul"])
            .spawn()
            .unwrap();
        let console = Console::new(shell.id());
        let mut busy = false;
        for _ in 0..50 {
            busy = console.is_busy();
            if busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let name = console.name();
        let _ = shell.kill();
        let _ = shell.wait();
        assert!(busy, "{name:?}");
        assert!(name.is_some_and(|name| name.eq_ignore_ascii_case("ping")));
    }

    #[test]
    fn a_process_that_is_gone_has_no_start() {
        assert!(start_time(std::process::id()).is_some());
        assert_eq!(start_time(u32::MAX - 2), None);
    }
}
