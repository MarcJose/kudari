// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What happens with a link that the user opens in a terminal.

use std::path::{Component, Path, Prefix};

use crate::command::{expand_home, split_command};
use crate::services::Desktop;

/// How to open a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Open {
    /// Through the platform, with the program the user chose for it.
    Url(String),
    /// A file at a line in the configured editor.
    Editor { program: String, arguments: Vec<String> },
}

/// Files go to the editor of the `EditorCommand` setting, with `PATH`, `LINE` and `COLUMN`
/// replaced by the parts of `/path:line:column`; everything else to the platform.
#[must_use]
pub fn resolve(link: &str, editor: &str) -> Open {
    let (path, line, column) = file_position(link);
    if path.starts_with('/')
        && let Some(mut words) = split_command(editor)
    {
        let replace = |word: &str| {
            word.replace("PATH", path)
                .replace("LINE", line.unwrap_or("1"))
                .replace("COLUMN", column.unwrap_or("1"))
        };
        let program = expand_home(&replace(&words.remove(0)));
        let arguments = words.iter().map(|word| replace(word)).collect();
        return Open::Editor { program, arguments };
    }
    Open::Url(path.to_string())
}

/// Whether a path names a share on the network, which Windows would log in to with the
/// credentials of the user, or a device. Windows takes any two slashes or backslashes at the
/// start as a share, and `\\?\UNC\` or `\\.\` as well.
#[must_use]
pub fn is_remote(path: &str) -> bool {
    let mut start = path.chars();
    let separator = |character: Option<char>| matches!(character, Some('/' | '\\'));
    if separator(start.next()) && separator(start.next()) {
        return true;
    }
    match Path::new(path).components().next() {
        Some(Component::Prefix(prefix)) => !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)),
        _ => false,
    }
}

/// The scheme of an address such as `https:`; none for a path, also not for `C:\`.
fn scheme(target: &str) -> Option<&str> {
    let (scheme, _) = target.split_once(':')?;
    let valid = scheme.len() > 1
        && scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character));
    valid.then_some(scheme)
}

/// What the platform may open for `target`, which text in a terminal can name: the address
/// itself if it has one of `schemes`, or the real path of a local folder or of a file that
/// starts no program. On Windows, opening hands it to the program the system has for it,
/// which would run programs and scripts as well.
///
/// The path is the one the file system gives back, so that what is checked is what opens:
/// Windows drops dots and spaces at the end of a name, which would let `run.exe.` pass for
/// a file without the extension of a program.
#[must_use]
pub fn openable(target: &str, schemes: &[String]) -> Option<String> {
    let path = match scheme(target) {
        Some(scheme) if !scheme.eq_ignore_ascii_case("file") => {
            return crate::view::is_safe_link(target, schemes).then(|| target.to_string());
        }
        // `file:///path`, with an empty host.
        Some(_) => target
            .get(7..)
            .filter(|rest| {
                rest.starts_with('/')
                    && target
                        .get(..7)
                        .is_some_and(|start| start.eq_ignore_ascii_case("file://"))
            })
            .and_then(crate::marks::percent_decoded)?,
        None => target.to_string(),
    };
    if is_remote(&path) || !Path::new(&path).is_absolute() || has_stream(&path) {
        return None;
    }
    let real = plain(&std::fs::canonicalize(&path).ok()?);
    if is_remote(&real) {
        return None;
    }
    let metadata = std::fs::metadata(&real).ok()?;
    (metadata.is_dir() || crate::view::is_inert(Path::new(&real), &metadata)).then_some(real)
}

/// Whether the platform may open `target`; see [`openable`].
#[must_use]
pub fn may_open(target: &str, schemes: &[String]) -> bool {
    openable(target, schemes).is_some()
}

/// Whether a Windows path names a stream of a file, `notes.txt:other`, which opens what the
/// stream holds. Unix allows colons in names.
fn has_stream(path: &str) -> bool {
    cfg!(windows)
        && Path::new(path)
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name.to_string_lossy().contains(':')))
}

/// A canonical path as programs expect it: Windows gives `\\?\C:\…` back.
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix("\\\\?\\") {
        Some(rest) if rest.get(1..2) == Some(":") => rest.to_string(),
        _ => text.into_owned(),
    }
}

/// The address of a web search for `words`: the `WebSearchUrl` setting with the words
/// encoded for an address in place of `%s`, or after it if it has none. `None` without a
/// search engine.
#[must_use]
pub fn web_search(template: &str, words: &str) -> Option<String> {
    use std::fmt::Write;

    let template = template.trim();
    if template.is_empty() {
        return None;
    }
    let mut encoded = String::with_capacity(words.len() * 3);
    for byte in words.trim().bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    Some(if template.contains("%s") {
        template.replace("%s", &encoded)
    } else {
        format!("{template}{encoded}")
    })
}

/// Opens a link, with `:line:column` after a file going to the configured editor. Anything
/// the platform may not open stays closed; see [`may_open`].
pub fn open(link: &str, editor: &str, schemes: &[String], desktop: &dyn Desktop) {
    match resolve(link, editor) {
        Open::Url(url) => match openable(&url, schemes) {
            Some(target) => desktop.open_url(&target),
            None => eprintln!("kudari: refused to open {url:?}"),
        },
        Open::Editor { program, arguments } => {
            let spawned = std::process::Command::new(program)
                .args(arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match spawned {
                // The child must not stay behind as a zombie.
                Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
                Err(error) => eprintln!("kudari: could not start the editor: {error}"),
            }
        }
    }
}

/// Splits `/path:12:3` into its parts, unless the whole text is an existing path.
fn file_position(link: &str) -> (&str, Option<&str>, Option<&str>) {
    if !link.starts_with('/') || std::path::Path::new(link).exists() {
        return (link, None, None);
    }
    let number = |text: &&str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let mut parts = link.rsplitn(3, ':');
    match (parts.next().filter(number), parts.next(), parts.next()) {
        (Some(column), Some(line), Some(path)) if number(&line) => (path, Some(line), Some(column)),
        (Some(line), Some(path), None) => (path, Some(line), None),
        (Some(line), Some(middle), Some(path)) => (&link[..path.len() + 1 + middle.len()], Some(line), None),
        _ => (link, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_positions_are_split_off_links() {
        assert_eq!(
            file_position("/nonexistent/a.rs:12:3"),
            ("/nonexistent/a.rs", Some("12"), Some("3"))
        );
        assert_eq!(
            file_position("/nonexistent/a.rs:12"),
            ("/nonexistent/a.rs", Some("12"), None)
        );
        assert_eq!(file_position("/nonexistent/a.rs"), ("/nonexistent/a.rs", None, None));
        assert_eq!(
            file_position("https://example.org:8080"),
            ("https://example.org:8080", None, None)
        );
    }

    #[test]
    fn files_go_to_the_editor_at_their_line() {
        assert_eq!(
            resolve("/nonexistent/a.rs:12:3", "kate --line LINE --column COLUMN PATH"),
            Open::Editor {
                program: "kate".into(),
                arguments: ["--line", "12", "--column", "3", "/nonexistent/a.rs"]
                    .map(String::from)
                    .to_vec(),
            }
        );
        assert_eq!(
            resolve("/nonexistent/a.rs", "vim +LINE PATH"),
            Open::Editor {
                program: "vim".into(),
                arguments: ["+1", "/nonexistent/a.rs"].map(String::from).to_vec(),
            }
        );
    }

    #[test]
    fn without_an_editor_or_for_addresses_the_platform_opens_the_link() {
        assert_eq!(
            resolve("/nonexistent/a.rs:12", ""),
            Open::Url("/nonexistent/a.rs".into())
        );
        assert_eq!(
            resolve("https://example.org:8080", "vim PATH"),
            Open::Url("https://example.org:8080".into())
        );
    }

    struct Opened(std::cell::RefCell<Vec<String>>);

    impl Desktop for Opened {
        fn open_url(&self, url: &str) {
            self.0.borrow_mut().push(url.to_string());
        }

        fn web_search(&self, _text: &str) {}
    }

    #[test]
    fn the_platform_gets_what_no_editor_takes() {
        let opened = Opened(std::cell::RefCell::default());
        let schemes = crate::options::Options::default().link_schemes;
        open("https://example.org", "vim PATH", &schemes, &opened);
        open("custom-handler://run", "vim PATH", &schemes, &opened);
        assert_eq!(*opened.0.borrow(), ["https://example.org"]);
    }

    #[test]
    fn only_allowed_schemes_and_plain_local_files_may_open() {
        let schemes = crate::options::Options::default().link_schemes;
        assert!(may_open("https://kde.org", &schemes));
        assert!(!may_open("custom-handler://run", &schemes));
        assert!(!may_open("ms-msdt:/id", &schemes));
        let directory = std::env::temp_dir().join(format!("kudari-open-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let notes = directory.join("notes.txt");
        std::fs::write(&notes, "x").unwrap();
        let notes = notes.to_string_lossy().into_owned();
        assert!(may_open(&notes, &schemes));
        assert!(may_open(&directory.to_string_lossy(), &schemes));
        assert!(!may_open(&directory.join("missing.txt").to_string_lossy(), &schemes));
        assert!(!may_open("notes.txt", &schemes), "a relative path");
        if cfg!(unix) {
            assert!(may_open(&format!("file://{notes}"), &schemes));
            assert!(may_open(&format!("FILE://{}", notes.replace(' ', "%20")), &schemes));
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn shares_on_the_network_never_open() {
        let mut schemes = crate::options::Options::default().link_schemes;
        schemes.push("file:".to_string());
        for target in [
            "\\\\server\\share\\a.txt",
            "//server/share/a.txt",
            "file://server/share/a.txt",
            "file:////server/share",
        ] {
            assert!(!may_open(target, &schemes), "{target}");
            assert!(is_remote(target) || target.starts_with("file:"), "{target}");
        }
    }

    #[test]
    fn web_searches_encode_their_words() {
        assert_eq!(
            web_search("https://duckduckgo.com/?q=%s", " rust & qt/ä "),
            Some("https://duckduckgo.com/?q=rust%20%26%20qt%2F%C3%A4".to_string())
        );
        assert_eq!(
            web_search("https://example.org/search?q=", "a b"),
            Some("https://example.org/search?q=a%20b".to_string())
        );
        assert_eq!(web_search("  ", "a"), None);
        let schemes = crate::options::Options::default().link_schemes;
        assert!(may_open(
            &web_search(crate::settings::DEFAULT_WEB_SEARCH, "x").unwrap(),
            &schemes
        ));
    }

    #[test]
    fn devices_and_verbatim_shares_are_remote() {
        assert!(is_remote("\\\\?\\UNC\\server\\share"));
        assert!(is_remote("\\\\.\\pipe\\name"));
        assert!(!is_remote("/home/ada"));
        assert!(!is_remote("C:\\Users"));
        assert_eq!(plain(Path::new("\\\\?\\C:\\Users")), "C:\\Users");
        assert_eq!(plain(Path::new("\\\\?\\UNC\\server")), "\\\\?\\UNC\\server");
        assert_eq!(plain(Path::new("/home/ada")), "/home/ada");
    }

    /// The tricks of Win32 names: what Windows opens is what is checked.
    #[test]
    #[cfg(windows)]
    fn windows_opens_only_what_was_checked() {
        let schemes = crate::options::Options::default().link_schemes;
        let directory = std::env::temp_dir().join(format!("kudari-open-windows-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["run.exe", "notes.txt", "script.py"] {
            std::fs::write(directory.join(name), "x").unwrap();
        }
        let path = |name: &str| directory.join(name).to_string_lossy().into_owned();
        assert!(openable(&path("notes.txt"), &schemes).is_some());
        assert_eq!(openable(&path("run.exe"), &schemes), None);
        assert_eq!(openable(&format!("{}.", path("run.exe")), &schemes), None);
        assert_eq!(openable(&format!("{} ", path("run.exe")), &schemes), None);
        assert_eq!(openable(&path("script.py"), &schemes), None);
        assert_eq!(openable(&format!("{}:stream", path("notes.txt")), &schemes), None);
        let opened = openable(&path("NOTES.TXT"), &schemes).unwrap();
        assert!(!opened.starts_with("\\\\?\\"), "{opened}");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn programs_never_open() {
        use std::os::unix::fs::PermissionsExt;
        let program = std::env::temp_dir().join(format!("kudari-open-program-{}", std::process::id()));
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let schemes = crate::options::Options::default().link_schemes;
        assert!(!may_open(&program.to_string_lossy(), &schemes));
        std::fs::remove_file(&program).unwrap();
    }

    #[test]
    fn random_links_do_no_harm() {
        for input in crate::fuzz::inputs(&["/usr/bin/a.rs:12:3", "https://example.org"]) {
            let _ = resolve(&input, "vim +LINE PATH");
        }
    }
}
