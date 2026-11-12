// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The programs of a terminal on Windows, from the list of processes. Windows tells neither
//! the working directory nor the command line of another process without reading its
//! memory, so the directory comes from the marks of the shell and the command line is the
//! name of the program alone.

use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    TerminateProcess,
};

use super::tree::{self, Process};
use super::{Programs, Signal};

/// The shell of a `ConPTY` and what it runs.
pub struct Console {
    shell: u32,
}

impl Console {
    /// The console whose shell has the process id `shell`.
    #[must_use]
    pub fn new(shell: u32) -> Self {
        Self { shell }
    }

    fn foreground(&self) -> (Vec<Process>, u32) {
        let processes = processes();
        let foreground = tree::foreground(&processes, self.shell);
        (processes, foreground)
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
            let process = OpenProcess(PROCESS_TERMINATE, 0, foreground);
            if !process.is_null() {
                TerminateProcess(process, 1);
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
