// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The mounts of the system, so that the text in a terminal does not mount drives. Looking at
//! a path under a mount point of autofs, as `/net/host` or a mount of systemd on demand,
//! mounts its drive first, and with it reaches a server: a program could make Kudari do that
//! with a path in its output, as soon as the pointer passes over it with Ctrl held.
#![cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "only Linux has mounts on demand to look for")
)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The path that `path` names with each symbolic link on the way followed, or nothing where
/// a step would go into a mount point of autofs with nothing mounted over it yet: looking
/// there would mount its drive. Each step is checked against the table of mounts before it
/// is looked at, so that neither the path nor a link on it, such as one in an unpacked
/// archive or `/proc/self/cwd`, can lead there.
#[must_use]
pub fn resolved(path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        resolved_in(&table::mounts(), path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Some(path.to_path_buf())
    }
}

#[cfg(target_os = "linux")]
mod table {
    use super::Mount;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How long the table counts as it is: mounts come and go rarely, and reading it is a
    /// read of `/proc` at every look.
    const FRESH: Duration = Duration::from_secs(1);

    static TABLE: Mutex<Option<(Instant, Arc<Vec<Mount>>)>> = Mutex::new(None);

    /// The table, read again when it is older than [`FRESH`]. Handed out, so that no look at
    /// a drive holds the lock.
    pub fn mounts() -> Arc<Vec<Mount>> {
        let mut table = TABLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match table.as_ref() {
            Some((read, mounts)) if read.elapsed() <= FRESH => Arc::clone(mounts),
            _ => {
                let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
                let mounts = Arc::new(super::parse(&text));
                *table = Some((Instant::now(), Arc::clone(&mounts)));
                mounts
            }
        }
    }
}

/// The most symbolic links one path may lead through, as Linux allows.
const MOST_LINKS: usize = 40;

/// [`resolved`] with the table `mounts`.
fn resolved_in(mounts: &[Mount], path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut steps: VecDeque<OsString> = VecDeque::new();
    let mut at = PathBuf::from("/");
    let mut links = 0;
    push_front(&mut steps, path);
    while let Some(step) = steps.pop_front() {
        match step.to_str() {
            Some("/") => at = PathBuf::from("/"),
            Some(".") => {}
            Some("..") => {
                at.pop();
            }
            _ => {
                let next = at.join(&step);
                if under_demand(mounts, &next) {
                    return None;
                }
                let metadata = std::fs::symlink_metadata(&next).ok()?;
                if metadata.is_symlink() {
                    links += 1;
                    if links > MOST_LINKS {
                        return None;
                    }
                    push_front(&mut steps, &std::fs::read_link(&next).ok()?);
                } else {
                    at = next;
                }
            }
        }
    }
    Some(at)
}

/// Puts the steps of `path` before `steps`: a `/` for its root, then each part.
fn push_front(steps: &mut VecDeque<OsString>, path: &Path) {
    let parts: Vec<OsString> = path
        .components()
        .map(|component| match component {
            Component::RootDir | Component::Prefix(_) => OsString::from("/"),
            Component::CurDir => OsString::from("."),
            Component::ParentDir => OsString::from(".."),
            Component::Normal(name) => name.to_os_string(),
        })
        .collect();
    for part in parts.into_iter().rev() {
        steps.push_front(part);
    }
}

/// A mount point, and whether it mounts on demand.
#[derive(Debug, PartialEq, Eq)]
struct Mount {
    point: PathBuf,
    on_demand: bool,
}

/// The mounts of a `/proc/self/mountinfo`, in its order: one mounted later over the same
/// point comes after the one below it.
fn parse(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let point = fields.get(4)?;
            // Optional fields come before the `-`, and the type of the file system after it.
            let separator = fields.iter().skip(6).position(|field| *field == "-")? + 6;
            let kind = fields.get(separator + 1)?;
            Some(Mount {
                point: PathBuf::from(unescaped(point)),
                on_demand: *kind == "autofs",
            })
        })
        .collect()
}

/// A field of the table, where the kernel writes a space, a tab, a line break and a
/// backslash as `\040`, `\011`, `\012` and `\134`.
fn unescaped(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let code = bytes
            .get(index + 1..index + 4)
            .filter(|digits| byte == b'\\' && digits.iter().all(|digit| (b'0'..=b'7').contains(digit)))
            .and_then(|digits| u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok());
        if let Some(code) = code {
            out.push(code);
            index += 4;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `path` lies under a mount on demand: the deepest mount point above it is one, and
/// of those at the same point, the last mounted.
fn under_demand(mounts: &[Mount], path: &Path) -> bool {
    let path = lexical(path);
    mounts
        .iter()
        .filter(|mount| path.starts_with(&mount.point))
        .fold(None::<&Mount>, |deepest, mount| match deepest {
            Some(deepest) if deepest.point.components().count() > mount.point.components().count() => Some(deepest),
            _ => Some(mount),
        })
        .is_some_and(|mount| mount.on_demand)
}

/// `path` with `.` and `..` taken out by its text, as the path is not looked at.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
22 1 0:21 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
40 22 0:35 / /net rw,relatime shared:20 - autofs /etc/auto.net rw,fd=7
41 40 0:36 / /net/nas rw,relatime shared:21 - nfs4 nas:/ rw
50 22 0:40 / /mnt/backup rw,relatime shared:30 - autofs systemd-1 rw,fd=50
51 50 0:41 / /mnt/backup rw,relatime shared:31 - ext4 /dev/sdb1 rw
60 22 0:45 / /mnt/on\\040demand rw,relatime - autofs systemd-1 rw
";

    #[test]
    fn a_path_under_a_mount_on_demand_is_not_looked_at() {
        let mounts = parse(TABLE);
        let on_demand = |path: &str| under_demand(&mounts, Path::new(path));
        assert!(on_demand("/net/other/file.txt"));
        assert!(on_demand("/net"));
        assert!(on_demand("/mnt/on demand/notes.txt"));
        assert!(on_demand("/home/../net/other/x"));
    }

    #[test]
    fn a_drive_mounted_already_is_looked_at() {
        let mounts = parse(TABLE);
        let on_demand = |path: &str| under_demand(&mounts, Path::new(path));
        assert!(!on_demand("/net/nas/file.txt"));
        // Mounted over the point of an automount of systemd: the later mount counts.
        assert!(!on_demand("/mnt/backup/x"));
        assert!(!on_demand("/home/user/notes.txt"));
        assert!(!on_demand("/network/x"));
        assert!(!on_demand("/net/other/../../home/x"));
    }

    /// A link on the way can lead into a mount on demand as well as the path itself: each
    /// step is checked before it is looked at.
    #[cfg(unix)]
    #[test]
    fn a_link_into_a_mount_on_demand_is_not_followed() {
        let root = std::env::temp_dir().join(format!("kudari-mounts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("real")).unwrap();
        // What the links lead to is there: only the check keeps them out.
        std::fs::create_dir_all(root.join("auto/host")).unwrap();
        std::fs::write(root.join("auto/host/x"), "").unwrap();
        std::fs::write(root.join("auto/x"), "").unwrap();
        std::fs::write(root.join("real/notes.txt"), "").unwrap();
        let link = |target: &str, name: &str| std::os::unix::fs::symlink(target, root.join(name)).unwrap();
        link("auto/host", "into");
        link(&root.join("auto").to_string_lossy(), "absolute");
        link("real/../auto", "around");
        link("real", "fine");
        link("again", "loop");
        std::os::unix::fs::symlink("loop", root.join("again")).unwrap();
        let mounts = [Mount {
            point: root.join("auto"),
            on_demand: true,
        }];
        let resolved = |path: &str| resolved_in(&mounts, &root.join(path));
        assert_eq!(resolved("into/x"), None);
        assert_eq!(resolved("absolute/x"), None);
        assert_eq!(resolved("around/x"), None);
        assert_eq!(resolved("real/../auto/x"), None);
        assert_eq!(resolved("loop/x"), None);
        assert_eq!(resolved("fine/notes.txt"), Some(root.join("real/notes.txt")));
        assert_eq!(resolved("real/./notes.txt"), Some(root.join("real/notes.txt")));
        assert_eq!(resolved_in(&mounts, Path::new("relative/notes.txt")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn broken_lines_count_as_no_mount() {
        assert_eq!(parse("22 1 0:21 / / rw\n\nshort line\n"), []);
        assert_eq!(unescaped("a\\040b\\134c\\04"), "a b\\c\\04");
    }
}
