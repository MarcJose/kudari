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
    /// The same for what is only shown, such as the mark of a busy tab: it may come from a
    /// look at the processes a moment ago, which a platform that has to list all processes
    /// for it shares between the terminals.
    fn is_busy_recent(&self) -> bool {
        self.is_busy()
    }
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
    /// Whether the program in the foreground runs as root, for `%B` in titles.
    fn is_root(&self) -> bool {
        false
    }
    /// Hangs up the shell and the program in the foreground, as a terminal that goes does,
    /// also when they are stopped.
    fn hang_up(&self) {}
    /// The programs as they are now, for several questions in a row: which program is in the
    /// foreground is found once for all of them, and they agree on it. What is read of it is
    /// still read only when asked. Not to keep: a moment later, another program can be there.
    fn now(&self) -> Box<dyn Programs + '_> {
        Box::new(Same(self))
    }
}

/// Programs that find their foreground for each question anew, as they are.
struct Same<'a, P: ?Sized>(&'a P);

impl<P: Programs + ?Sized> Programs for Same<'_, P> {
    fn is_busy(&self) -> bool {
        self.0.is_busy()
    }
    fn is_busy_recent(&self) -> bool {
        self.0.is_busy_recent()
    }
    fn working_directory(&self) -> Option<PathBuf> {
        self.0.working_directory()
    }
    fn name(&self) -> Option<String> {
        self.0.name()
    }
    fn arguments(&self) -> Option<Vec<String>> {
        self.0.arguments()
    }
    fn signal(&self, signal: Signal) {
        self.0.signal(signal);
    }
    fn supports(&self, signal: Signal) -> bool {
        self.0.supports(signal)
    }
    fn set_flow_control(&self, enabled: bool) {
        self.0.set_flow_control(enabled);
    }
    fn is_root(&self) -> bool {
        self.0.is_root()
    }
    fn hang_up(&self) {
        self.0.hang_up();
    }
    fn now(&self) -> Box<dyn Programs + '_> {
        Box::new(Same(self.0))
    }
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

/// The name of this computer, as it is now: it can change while Kudari runs.
#[must_use]
pub fn host() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(unix)]
    let name = {
        // A name has at most 64 bytes on Linux, and 255 by POSIX.
        let mut name = [0u8; 256];
        // SAFETY: the buffer is valid for the length given, which leaves its last byte zero,
        // so the name ends with a zero byte also when it is cut.
        let got = unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len() - 1) };
        let name = if got == 0 {
            name.split(|byte| *byte == 0).next().unwrap_or_default()
        } else {
            &[]
        };
        String::from_utf8_lossy(name).into_owned()
    };
    name.trim().to_string()
}

/// Writes a file that only the user can read, since terminal output can hold passwords and
/// tokens. A file that already exists keeps its permissions. On Windows, the folders of the
/// user already keep others out.
///
/// The new content goes to a file beside it, which then takes its place: until then the old
/// file stays whole, also when Kudari ends or the disk is full while it writes. A link stays
/// a link, and the file it names is replaced. The new file gets the access of the old one,
/// with its group and its access control list; what cannot be taken over leaves it to its
/// owner alone, never with more access than before.
///
/// # Errors
/// What the file system reports.
pub fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = target
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    let mut partial_name = std::ffi::OsString::from(".");
    partial_name.push(name);
    partial_name.push(format!(".{}.partial", std::process::id()));
    let partial = target.with_file_name(partial_name);
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&partial)?;
        file.write_all(content)?;
        // On the disk before it takes the place of the old file, or a crash just after can
        // leave an empty file where the old one was.
        file.sync_all()?;
        replace(&partial, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// Puts `partial` in the place of `target`, with the access of `target` if it exists.
#[cfg(unix)]
fn replace(partial: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if let Ok(metadata) = std::fs::metadata(target) {
        let mode = metadata.permissions().mode() & 0o7777;
        let group = std::os::unix::fs::chown(partial, None, Some(metadata.gid())).is_ok();
        std::fs::set_permissions(partial, std::fs::Permissions::from_mode(mode))?;
        // The list comes after the mode, which would change its mask.
        if !group || !copy_access_list(target, partial) {
            std::fs::set_permissions(partial, std::fs::Permissions::from_mode(mode & 0o700))?;
        }
    }
    std::fs::rename(partial, target)
}

/// Gives `to` the POSIX access control list of `from`; true if there was none or it went
/// over.
#[cfg(unix)]
fn copy_access_list(from: &Path, to: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;

        let name = c"system.posix_acl_access";
        let (Ok(from), Ok(to)) = (
            std::ffi::CString::new(from.as_os_str().as_bytes()),
            std::ffi::CString::new(to.as_os_str().as_bytes()),
        ) else {
            return false;
        };
        // SAFETY: names that end in zero, and a buffer of the size that the first call tells.
        unsafe {
            let size = libc::getxattr(from.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0);
            if size < 0 {
                let error = std::io::Error::last_os_error().raw_os_error();
                return matches!(error, Some(libc::ENODATA | libc::ENOTSUP));
            }
            let mut list = vec![0u8; size.unsigned_abs()];
            let read = libc::getxattr(from.as_ptr(), name.as_ptr(), list.as_mut_ptr().cast(), list.len());
            read >= 0 && libc::setxattr(to.as_ptr(), name.as_ptr(), list.as_ptr().cast(), read.unsigned_abs(), 0) == 0
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (from, to);
        true
    }
}

/// Puts `partial` in the place of `target`. An existing file is replaced as Windows does it
/// for editors: it keeps its access control list and its attributes.
#[cfg(windows)]
fn replace(partial: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};

    if !target.exists() {
        return std::fs::rename(partial, target);
    }
    let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(std::iter::once(0)).collect() };
    let (target, partial) = (wide(target), wide(partial));
    // SAFETY: names that end in zero; the other arguments may be null.
    let replaced = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            partial.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    #[cfg(unix)]
    fn the_old_file_stays_whole_until_the_new_one_is_written() {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!("kudari-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("save.txt");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        // What reads the file while it is saved, or what a crash would leave, sees the old
        // content whole: the new one is a file of its own.
        let mut reader = std::fs::File::open(&path).unwrap();
        write_private(&path, b"new").unwrap();
        let mut seen = String::new();
        reader.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "old");
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        // A link stays a link.
        let link = directory.join("link.txt");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        write_private(&link, b"through the link").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&path).unwrap(), b"through the link");
        // An access control list stays, as `setfacl` sets it.
        let listed = std::process::Command::new("setfacl")
            .args(["-m", "u:nobody:r"])
            .arg(&path)
            .status()
            .is_ok_and(|status| status.success());
        if listed {
            write_private(&path, b"listed").unwrap();
            let list = std::process::Command::new("getfacl").arg(&path).output().unwrap();
            assert!(String::from_utf8_lossy(&list.stdout).contains("user:nobody:r--"));
        }
        // Nothing stays beside it.
        let names: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// An access control list set with `icacls` stays when the file is written again.
    #[test]
    #[cfg(windows)]
    fn windows_keeps_the_access_of_a_file_written_again() {
        let path = std::env::temp_dir().join(format!("kudari-access-{}.txt", std::process::id()));
        std::fs::write(&path, b"old").unwrap();
        let granted = std::process::Command::new("icacls")
            .arg(&path)
            .args(["/grant", "*S-1-5-32-546:(R)"])
            .output()
            .unwrap();
        assert!(granted.status.success());
        write_private(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let list = std::process::Command::new("icacls").arg(&path).output().unwrap();
        let list = String::from_utf8_lossy(&list.stdout).to_string();
        assert!(list.contains("Guests") || list.contains("S-1-5-32-546"), "{list}");
        std::fs::remove_file(&path).unwrap();
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
