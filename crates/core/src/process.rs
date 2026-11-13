// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What the operating system tells about the programs in a terminal, the signals it sends
//! them, and the little else Kudari asks of it: the user, the host, the home directory and
//! files only the user can read. Linux answers through `/proc` and the PTY, Windows
//! through the process tree of the console.

use std::path::{Path, PathBuf};

pub mod tree;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::Pty;
#[cfg(windows)]
pub use windows::Console;

/// A signal for the programs in the foreground, as the terminal menu of Konsole offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Stop,
    Continue,
    Hangup,
    Interrupt,
    Terminate,
    Kill,
    User1,
    User2,
}

impl Signal {
    /// The signal with this number on Linux.
    #[must_use]
    pub fn from_number(number: i32) -> Option<Self> {
        Some(match number {
            19 => Self::Stop,
            18 => Self::Continue,
            1 => Self::Hangup,
            2 => Self::Interrupt,
            15 => Self::Terminate,
            9 => Self::Kill,
            10 => Self::User1,
            12 => Self::User2,
            _ => return None,
        })
    }
}

/// The programs in one terminal: its shell and what the shell runs in the foreground.
pub trait Programs: Send + Sync {
    /// Whether a program other than the shell runs in the foreground.
    fn is_busy(&self) -> bool;
    /// The working directory of the program in the foreground, or else of the shell.
    fn working_directory(&self) -> Option<PathBuf>;
    /// The name of the program in the foreground.
    fn name(&self) -> Option<String>;
    /// The command line of the program in the foreground, starting with the program.
    fn arguments(&self) -> Option<Vec<String>>;
    fn signal(&self, signal: Signal);
    /// Whether [`Programs::signal`] can send `signal`, so that menus offer only those.
    fn supports(&self, signal: Signal) -> bool;
    /// Whether Ctrl+S and Ctrl+Q stop and resume the output.
    fn set_flow_control(&self, enabled: bool);
}

/// The home directory of the user.
#[must_use]
pub fn home() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// The login name of the user.
#[must_use]
pub fn user() -> String {
    let name = if cfg!(windows) { "USERNAME" } else { "USER" };
    std::env::var(name).unwrap_or_default()
}

/// The name of this computer.
#[must_use]
pub fn host() -> String {
    let name = if cfg!(windows) {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    } else {
        std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default()
    };
    name.trim().to_string()
}

/// Writes a file that only the user can read, since terminal output can hold passwords and
/// tokens. A file that already exists keeps its permissions. On Windows, the folders of the
/// user already keep others out.
///
/// # Errors
/// What the file system reports.
pub fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(content)
}

/// The name of the program behind process `pid`, for the question about an AI agent. For
/// `kudari mcp`, the binary `own`, that is the agent which started it. Programs choose
/// their own names, so this is only a hint.
#[must_use]
pub fn program(pid: u32, own: Option<&Path>) -> Option<String> {
    #[cfg(unix)]
    return unix::program(Path::new("/proc"), pid, own);
    #[cfg(windows)]
    return windows::program(pid, own);
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, own);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signals_of_the_menu_are_known() {
        for number in [19, 18, 1, 2, 15, 9, 10, 12] {
            assert!(Signal::from_number(number).is_some(), "{number}");
        }
        assert_eq!(Signal::from_number(2), Some(Signal::Interrupt));
        assert_eq!(Signal::from_number(0), None);
        assert_eq!(Signal::from_number(-1), None);
    }

    #[test]
    fn private_files_are_written() {
        let path = std::env::temp_dir().join(format!("kudari-private-{}", std::process::id()));
        write_private(&path, b"secret").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_system_has_names() {
        let _ = (user(), host(), home());
    }
}
