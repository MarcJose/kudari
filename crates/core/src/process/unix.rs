// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The programs of a terminal on Linux: the foreground process group of the PTY, and what
//! `/proc` tells about it.

use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

use super::{Programs, Signal};

/// The PTY of a terminal and the shell on it.
pub struct Pty {
    /// A descriptor of its own of the PTY: the loop of the terminal closes its one when it
    /// ends, and the number of that one goes to the next terminal that opens.
    fd: Option<OwnedFd>,
    shell: i32,
    proc: PathBuf,
}

impl Pty {
    /// The PTY of `file`, and the process id of its shell.
    ///
    /// # Errors
    /// When no descriptor is left for a copy of the one of `file`.
    pub fn of(file: &impl AsFd, shell: i32) -> std::io::Result<Self> {
        Ok(Self {
            fd: Some(file.as_fd().try_clone_to_owned()?),
            shell,
            proc: PathBuf::from("/proc"),
        })
    }

    fn raw(&self) -> RawFd {
        self.fd.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    /// The process group in the foreground of the PTY, if it tells one.
    fn foreground_group(&self) -> Option<i32> {
        // SAFETY: plain system call; a descriptor of no terminal only makes it fail.
        let group = unsafe { libc::tcgetpgrp(self.raw()) };
        (group > 0).then_some(group)
    }

    /// The process group in the foreground of the PTY, or the shell.
    fn foreground(&self) -> i32 {
        self.foreground_group().unwrap_or(self.shell)
    }

    fn file(&self, pid: i32, name: &str) -> PathBuf {
        self.proc.join(pid.to_string()).join(name)
    }
}

/// Whether the effective user in the `Uid:` line of `/proc/<pid>/status` is root.
fn is_root_status(status: &str) -> bool {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1))
        == Some("0")
}

impl Programs for Pty {
    fn is_root(&self) -> bool {
        std::fs::read_to_string(self.file(self.foreground(), "status")).is_ok_and(|status| is_root_status(&status))
    }

    fn is_busy(&self) -> bool {
        self.foreground() != self.shell
    }

    fn working_directory(&self) -> Option<PathBuf> {
        std::fs::read_link(self.file(self.foreground(), "cwd"))
            .or_else(|_| std::fs::read_link(self.file(self.shell, "cwd")))
            .ok()
    }

    fn name(&self) -> Option<String> {
        std::fs::read_to_string(self.file(self.foreground(), "comm"))
            .ok()
            .map(|name| name.trim().to_string())
    }

    fn arguments(&self) -> Option<Vec<String>> {
        let command_line = std::fs::read(self.file(self.foreground(), "cmdline")).ok()?;
        let mut arguments: Vec<String> = command_line
            .split(|byte| *byte == 0)
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        // The arguments end with a zero byte each.
        if arguments.last().is_some_and(String::is_empty) {
            arguments.pop();
        }
        Some(arguments)
    }

    fn signal(&self, signal: Signal) {
        let number = match signal {
            Signal::Stop => libc::SIGSTOP,
            Signal::Continue => libc::SIGCONT,
            Signal::Hangup => libc::SIGHUP,
            Signal::Interrupt => libc::SIGINT,
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::User1 => libc::SIGUSR1,
            Signal::User2 => libc::SIGUSR2,
        };
        // Only to a group that the PTY tells: the id of the shell can belong to another
        // process by now, once the shell ended.
        if let Some(group) = self.foreground_group() {
            // SAFETY: plain system call; the process group may be gone, which only fails.
            unsafe { libc::killpg(group, number) };
        }
    }

    fn supports(&self, _signal: Signal) -> bool {
        true
    }

    fn set_flow_control(&self, enabled: bool) {
        // SAFETY: `attributes` is valid for both calls, and a stale descriptor only makes
        // them fail.
        unsafe {
            let mut attributes = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(self.raw(), &raw mut attributes) == 0 {
                if enabled {
                    attributes.c_iflag |= libc::IXON;
                } else {
                    attributes.c_iflag &= !libc::IXON;
                }
                libc::tcsetattr(self.raw(), libc::TCSANOW, &raw const attributes);
            }
        }
    }
}

/// The name of the program behind process `pid`. For `kudari mcp`, the binary `own`, that
/// is the agent which started it. Programs choose their own names, so this is only a hint.
pub(super) fn program(proc: &Path, pid: u32, own: Option<&Path>) -> Option<String> {
    let process = proc.join(pid.to_string());
    let is_own = own.is_some_and(|own| std::fs::read_link(process.join("exe")).is_ok_and(|exe| exe == own));
    let process = if is_own {
        let stat = std::fs::read_to_string(process.join("stat")).ok()?;
        // The name in parentheses can contain spaces; the parent follows the state.
        let parent: u32 = stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()?;
        proc.join(parent.to_string())
    } else {
        process
    };
    let name: String = std::fs::read_to_string(process.join("comm"))
        .ok()?
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_effective_user_tells_root() {
        assert!(is_root_status("Name:\tsudo\nUid:\t1000\t0\t0\t0\n"));
        assert!(!is_root_status("Uid:\t0\t1000\t1000\t1000\n"));
        assert!(!is_root_status("Name:\tbash\n"));
    }

    /// A fake `/proc` with one process, the shell, since no descriptor means no foreground.
    fn shell(root: &Path) -> Pty {
        let directory = root.join("42");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("comm"), "zsh\n").unwrap();
        std::fs::write(directory.join("cmdline"), b"ssh\0-p\x002222\0ada@box\0").unwrap();
        std::os::unix::fs::symlink("/work", directory.join("cwd")).unwrap();
        Pty {
            fd: None,
            shell: 42,
            proc: root.to_path_buf(),
        }
    }

    /// A new PTY, its master and its other end.
    fn open_pty() -> (std::fs::File, std::fs::File) {
        let (mut master, mut other) = (-1, -1);
        // SAFETY: two out parameters; the names and settings may be null.
        let opened = unsafe {
            libc::openpty(
                &raw mut master,
                &raw mut other,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(opened, 0);
        // SAFETY: new descriptors that nothing else owns.
        unsafe {
            (
                std::os::fd::FromRawFd::from_raw_fd(master),
                std::os::fd::FromRawFd::from_raw_fd(other),
            )
        }
    }

    #[test]
    fn the_number_of_the_pty_stays_its_own() {
        let (master, _other) = open_pty();
        let pty = Pty::of(&master, 1).unwrap();
        let number = master.as_raw_fd();
        // The loop of the terminal ends and closes its descriptor, whose number the next
        // terminal that opens can get. The one of the PTY is another, and still open.
        drop(master);
        assert_ne!(pty.raw(), number);
        // SAFETY: a plain query of a descriptor.
        assert_ne!(unsafe { libc::fcntl(pty.raw(), libc::F_GETFD) }, -1);
    }

    #[test]
    fn signals_go_only_to_a_group_that_the_pty_tells() {
        use std::os::unix::process::CommandExt;

        // A PTY whose group is not known, and a process with the id of its shell, which
        // leads its group of processes, as a shell does.
        let mut process = std::process::Command::new("sleep")
            .arg("5")
            .process_group(0)
            .spawn()
            .unwrap();
        let (reading, _writing) = std::io::pipe().unwrap();
        let pty = Pty::of(&reading, process.id() as i32).unwrap();
        pty.signal(Signal::Kill);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let alive = process.try_wait().unwrap().is_none();
        let _ = process.kill();
        let _ = process.wait();
        assert!(alive, "the process with the id of the shell was ended");
    }

    #[test]
    fn proc_tells_about_the_shell() {
        let root = std::env::temp_dir().join(format!("kudari-pty-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pty = shell(&root);
        assert!(!pty.is_busy());
        assert_eq!(pty.name().as_deref(), Some("zsh"));
        assert_eq!(pty.working_directory(), Some(PathBuf::from("/work")));
        assert_eq!(pty.arguments().unwrap(), ["ssh", "-p", "2222", "ada@box"]);
        assert!(pty.supports(Signal::User2));
        // Without a PTY, these only fail.
        pty.set_flow_control(true);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_program_behind_kudari_mcp_is_its_parent() {
        let root = std::env::temp_dir().join(format!("kudari-agent-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let own = root.join("kudari");
        let process = |pid: u32, comm: &str, exe: &Path, stat: &str| {
            let directory = root.join(pid.to_string());
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("comm"), comm).unwrap();
            std::fs::write(directory.join("stat"), stat).unwrap();
            std::os::unix::fs::symlink(exe, directory.join("exe")).unwrap();
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&own, "").unwrap();
        process(10, "claude\n", Path::new("/usr/bin/node"), "10 (claude) S 1 10 10");
        process(11, "kudari\n", &own, "11 (kudari (mcp)) S 10 11 11");
        process(12, "evil\x1b[2J\n", Path::new("/tmp/evil"), "12 (evil) S 1 12 12");
        process(13, "\n", Path::new("/tmp/blank"), "13 () S 1 13 13");
        assert_eq!(program(&root, 11, Some(&own)).as_deref(), Some("claude"));
        assert_eq!(program(&root, 11, None).as_deref(), Some("kudari"));
        assert_eq!(program(&root, 10, Some(&own)).as_deref(), Some("claude"));
        assert_eq!(program(&root, 12, Some(&own)).as_deref(), Some("evil[2J"));
        assert_eq!(program(&root, 13, Some(&own)), None);
        assert_eq!(program(&root, 99, Some(&own)), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
