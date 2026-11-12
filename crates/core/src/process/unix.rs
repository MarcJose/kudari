// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The programs of a terminal on Linux: the foreground process group of the PTY, and what
//! `/proc` tells about it.

use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use super::{Programs, Signal};

/// The PTY of a terminal and the shell on it.
pub struct Pty {
    fd: RawFd,
    shell: i32,
    proc: PathBuf,
}

impl Pty {
    /// The PTY with the descriptor `fd`, which must stay open as long as this lives or the
    /// calls on it fail, and the process id of its shell.
    #[must_use]
    pub fn new(fd: RawFd, shell: i32) -> Self {
        Self {
            fd,
            shell,
            proc: PathBuf::from("/proc"),
        }
    }

    /// The process group in the foreground of the PTY, or the shell.
    fn foreground(&self) -> i32 {
        // SAFETY: plain system call; a stale descriptor only makes it fail, which falls
        // back to the shell.
        let group = unsafe { libc::tcgetpgrp(self.fd) };
        if group > 0 { group } else { self.shell }
    }

    fn file(&self, pid: i32, name: &str) -> PathBuf {
        self.proc.join(pid.to_string()).join(name)
    }
}

impl Programs for Pty {
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
        // SAFETY: plain system call; the process group may be gone, which only fails.
        unsafe { libc::killpg(self.foreground(), number) };
    }

    fn supports(&self, _signal: Signal) -> bool {
        true
    }

    fn set_flow_control(&self, enabled: bool) {
        // SAFETY: `attributes` is valid for both calls, and a stale descriptor only makes
        // them fail.
        unsafe {
            let mut attributes = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(self.fd, &raw mut attributes) == 0 {
                if enabled {
                    attributes.c_iflag |= libc::IXON;
                } else {
                    attributes.c_iflag &= !libc::IXON;
                }
                libc::tcsetattr(self.fd, libc::TCSANOW, &raw const attributes);
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

    /// A fake `/proc` with one process, the shell, since no descriptor means no foreground.
    fn shell(root: &Path) -> Pty {
        let directory = root.join("42");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("comm"), "zsh\n").unwrap();
        std::fs::write(directory.join("cmdline"), b"ssh\0-p\x002222\0ada@box\0").unwrap();
        std::os::unix::fs::symlink("/work", directory.join("cwd")).unwrap();
        Pty {
            fd: -1,
            shell: 42,
            proc: root.to_path_buf(),
        }
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
