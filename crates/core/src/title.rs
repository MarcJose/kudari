// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The titles of terminals, with the placeholders of Konsole, so that formats carry over.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use crate::process::{self, Programs};
use crate::sessions::SessionId;

/// The title of a terminal: `remote_format` while `ssh` runs in the foreground, unless it
/// is empty, else `format`. `directory` gives the working directory, as far as it is known;
/// `window_title` is the title the program set. Only what the format shows is read, each once,
/// so `programs` is best a look at them [`Programs::now`].
#[must_use]
pub fn title(
    programs: &dyn Programs,
    directory: &dyn Fn() -> Option<PathBuf>,
    format: &str,
    remote_format: &str,
    window_title: &str,
) -> String {
    let look = Look::new(programs, directory);
    // The name of the program is one file, its command line another; most programs are no ssh.
    if !remote_format.is_empty()
        && look.name().eq_ignore_ascii_case("ssh")
        && let Some(target) = ssh_target(programs)
    {
        return format_remote_title(remote_format, &target, window_title);
    }
    fill_in_local(&look, format, window_title)
}

/// The text of a format string; see [`fill_in_local`].
#[must_use]
pub fn local_title(
    programs: &dyn Programs,
    directory: &dyn Fn() -> Option<PathBuf>,
    format: &str,
    window_title: &str,
) -> String {
    fill_in_local(&Look::new(programs, directory), format, window_title)
}

/// The target of an `ssh` running in the foreground.
fn ssh_target(programs: &dyn Programs) -> Option<SshTarget> {
    let arguments = programs.arguments()?;
    let program = Path::new(arguments.first()?).file_name()?.to_str()?;
    matches!(program, "ssh" | "ssh.exe").then(|| parse_ssh(arguments.get(1..)?))?
}

/// What a title can show of the programs of a terminal, each read when a format first asks
/// for it.
struct Look<'a> {
    programs: &'a dyn Programs,
    find_directory: &'a dyn Fn() -> Option<PathBuf>,
    name: OnceCell<String>,
    directory: OnceCell<Option<PathBuf>>,
    home: OnceCell<Option<PathBuf>>,
}

impl<'a> Look<'a> {
    fn new(programs: &'a dyn Programs, find_directory: &'a dyn Fn() -> Option<PathBuf>) -> Self {
        Self {
            programs,
            find_directory,
            name: OnceCell::new(),
            directory: OnceCell::new(),
            home: OnceCell::new(),
        }
    }

    fn name(&self) -> &str {
        self.name.get_or_init(|| self.programs.name().unwrap_or_default())
    }

    fn directory(&self) -> Option<&Path> {
        self.directory.get_or_init(self.find_directory).as_deref()
    }

    fn home(&self) -> Option<&Path> {
        self.home.get_or_init(process::home).as_deref()
    }
}

/// Fills in a format string: `%` and a letter that `value` knows becomes its text, `%%`
/// becomes `%`, and anything else stays as written.
fn fill_in(format: &str, value: impl Fn(char) -> Option<String>) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(position) = rest.find('%') {
        out.push_str(&rest[..position]);
        let after = &rest[position + 1..];
        match after.chars().next() {
            Some('%') => {
                out.push('%');
                rest = &after[1..];
            }
            Some(letter) => {
                if let Some(text) = value(letter) {
                    out.push_str(&text);
                } else {
                    out.push('%');
                    out.push(letter);
                }
                rest = &after[letter.len_utf8()..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The title of a local terminal. The placeholders are Konsole's, so that formats carry over:
/// `%d` directory (short), `%D` directory (full), `%n` program, `%u` user, `%h` host, `%B`
/// `#` for root and else `$`, and `%w` the title set by the program; `%#`, the number of the
/// session, comes in before with [`with_session`].
fn fill_in_local(look: &Look<'_>, format: &str, window_title: &str) -> String {
    fill_in(format, |letter| {
        Some(match letter {
            'd' => look
                .directory()
                .map_or_else(String::new, |path| short_directory(path, look.home())),
            'D' => look
                .directory()
                .map_or_else(String::new, |path| full_directory(path, look.home())),
            'n' => look.name().to_string(),
            'u' => process::user(),
            'h' => process::host(),
            'w' => window_title.to_string(),
            'B' if look.programs.is_root() => "#".to_string(),
            'B' => "$".to_string(),
            _ => return None,
        })
    })
}

/// Where an `ssh` command line connects to.
#[derive(Debug, Default, PartialEq)]
pub struct SshTarget {
    user: String,
    host: String,
    port: String,
    command: String,
}

/// A host and its port, as `box:2222` or `[::1]:2222` in an address of ssh.
fn split_port(host: &str) -> (&str, Option<&str>) {
    let is_port = |port: &&str| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit());
    if let Some(rest) = host.strip_prefix('[')
        && let Some((address, after)) = rest.split_once(']')
    {
        return (address, after.strip_prefix(':').filter(is_port));
    }
    match host.rsplit_once(':') {
        Some((name, port)) if is_port(&port) => (name, Some(port)),
        _ => (host, None),
    }
}

/// `format` with `%#` replaced by the number of `session`, counted from 1 as in Konsole: it
/// stays when tabs move. `%%` stays as it is, for the format to fill in afterwards.
#[must_use]
pub fn with_session(format: &str, session: SessionId) -> String {
    let mut out = String::with_capacity(format.len());
    let mut characters = format.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('#') => out.push_str(&(i64::from(session) + 1).to_string()),
            Some(next) => {
                out.push('%');
                out.push(next);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Reads the arguments of `ssh`, with the option letters of its manual.
fn parse_ssh(arguments: &[String]) -> Option<SshTarget> {
    const WITH_VALUE: &str = "BbcDeEFIiJLlmOoPpQRSWw";
    let mut target = SshTarget::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            // `ssh://user@host:port`, where the port can follow the host; elsewhere a colon
            // belongs to an address of IPv6, as in `ada@fe80::1`.
            let uri = word.strip_prefix("ssh://");
            let word = uri.map_or(word.as_str(), |rest| rest.split('/').next().unwrap_or_default());
            let (user, host) = word.rsplit_once('@').map_or(("", word), |(user, host)| (user, host));
            if !user.is_empty() {
                target.user = user.to_string();
            }
            let (host, port) = match uri.map(|_| split_port(host)) {
                Some((host, port)) => (host, port),
                None => (host, None),
            };
            target.host = host.to_string();
            // The port of -p comes before the one of the address.
            if let Some(port) = port.filter(|_| target.port.is_empty()) {
                target.port = port.to_string();
            }
            target.command = words.map(String::as_str).collect::<Vec<_>>().join(" ");
            return Some(target);
        };
        for (index, option) in options.char_indices() {
            if !WITH_VALUE.contains(option) {
                continue;
            }
            let rest = &options[index + option.len_utf8()..];
            let value = if rest.is_empty() {
                words.next()?.clone()
            } else {
                rest.to_string()
            };
            match option {
                'l' => target.user = value,
                'p' => target.port = value,
                _ => {}
            }
            break;
        }
    }
    None
}

/// The title of a terminal running `ssh`, with Konsole's placeholders for that case: `%u`
/// user, `%U` user and `@` if there is a user, `%h` host without its domain, `%H` host and
/// port, `%c` the remote command and `%w` the title set by the program.
fn format_remote_title(format: &str, target: &SshTarget, window_title: &str) -> String {
    fill_in(format, |letter| {
        Some(match letter {
            'u' => target.user.clone(),
            'U' if target.user.is_empty() => String::new(),
            'U' => format!("{}@", target.user),
            // An address has no domain to leave out.
            'h' if target.host.parse::<std::net::IpAddr>().is_ok() => target.host.clone(),
            'h' => target.host.split('.').next().unwrap_or_default().to_string(),
            'H' if target.port.is_empty() => target.host.clone(),
            'H' => format!("{}:{}", target.host, target.port),
            'c' => target.command.clone(),
            'w' => window_title.to_string(),
            _ => return None,
        })
    })
}

fn full_directory(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `~` for the home directory, otherwise the last path component.
fn short_directory(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    if Some(path) == home {
        return "~".to_string();
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::process::Signal;

    struct Ssh(Vec<String>);

    impl Programs for Ssh {
        fn is_busy(&self) -> bool {
            true
        }

        fn working_directory(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/srv/src"))
        }

        fn name(&self) -> Option<String> {
            Some("ssh".into())
        }

        fn arguments(&self) -> Option<Vec<String>> {
            Some(self.0.clone())
        }

        fn signal(&self, _signal: Signal) {}

        fn supports(&self, _signal: Signal) -> bool {
            false
        }

        fn set_flow_control(&self, _enabled: bool) {}
    }

    fn ssh(line: &str) -> Ssh {
        Ssh(line.split(' ').map(str::to_string).collect())
    }

    /// A shell, run as root or not.
    struct Shell(bool);

    impl Programs for Shell {
        fn is_busy(&self) -> bool {
            false
        }

        fn working_directory(&self) -> Option<PathBuf> {
            None
        }

        fn name(&self) -> Option<String> {
            Some("bash".into())
        }

        fn arguments(&self) -> Option<Vec<String>> {
            Some(vec!["bash".into()])
        }

        fn signal(&self, _signal: Signal) {}

        fn supports(&self, _signal: Signal) -> bool {
            false
        }

        fn set_flow_control(&self, _enabled: bool) {}

        fn is_root(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn konsoles_session_number_and_prompt_character_are_known() {
        // As an imported profile of Konsole has it.
        assert_eq!(with_session("%# %d", 0), "1 %d");
        assert_eq!(with_session("%#:%#", 4), "5:5");
        assert_eq!(with_session("100%%# %", 0), "100%%# %");
        assert_eq!(
            local_title(&Shell(false), &|| None, &with_session("%# %n %B", 2), ""),
            "3 bash $"
        );
        assert_eq!(local_title(&Shell(true), &|| None, "%n %B", ""), "bash #");
        // A placeholder escaped with %% stays text.
        assert_eq!(
            local_title(&Shell(true), &|| None, &with_session("%%# %%B", 0), ""),
            "%# %B"
        );
    }

    #[test]
    fn ssh_gets_the_remote_title() {
        assert_eq!(
            title(&ssh("/usr/bin/ssh -p 2222 ada@box"), &|| None, "%d", "%U%h", ""),
            "ada@box"
        );
        assert_eq!(
            title(&ssh("ssh.exe box uptime"), &|| None, "%d", "%h %c", ""),
            "box uptime"
        );
        assert_eq!(
            title(&ssh("ssh box"), &|| Some(PathBuf::from("/srv/src")), "%d : %n", "", ""),
            "src : ssh"
        );
        assert_eq!(title(&ssh("sshd -D"), &|| None, "%n %w", "%h", "notes"), "ssh notes");
    }

    #[test]
    fn ssh_command_lines_are_understood() {
        let parse = |line: &str| parse_ssh(&line.split(' ').map(str::to_string).collect::<Vec<_>>());
        let target = parse("-p 2222 -A ada@box.example.org uptime -p").unwrap();
        assert_eq!(target.user, "ada");
        assert_eq!(target.host, "box.example.org");
        assert_eq!(target.port, "2222");
        assert_eq!(target.command, "uptime -p");
        assert_eq!(parse("-l root -i key 10.0.0.1").unwrap().user, "root");
        assert_eq!(parse("-oBatchMode=yes host").unwrap().host, "host");
        assert_eq!(parse("-v"), None);
        // -B and -P take a value.
        let bound = parse("-B eth0 server").unwrap();
        assert_eq!((bound.host.as_str(), bound.command.as_str()), ("server", ""));
        assert_eq!(parse("-P work box").unwrap().host, "box");
        // An address of ssh, with its port, and -p before it.
        let uri = parse("ssh://ada@box:2222/").unwrap();
        assert_eq!(
            (uri.user.as_str(), uri.host.as_str(), uri.port.as_str()),
            ("ada", "box", "2222")
        );
        let ipv6 = parse("ssh://[::1]:2200").unwrap();
        assert_eq!((ipv6.host.as_str(), ipv6.port.as_str()), ("::1", "2200"));
        assert_eq!(parse("-p 22 ssh://box:2222").unwrap().port, "22");
        // Without ssh://, a colon is part of the host.
        assert_eq!(parse("ada@fe80::1").unwrap().host, "fe80::1");
    }

    #[test]
    fn remote_titles_use_konsoles_placeholders() {
        let target = SshTarget {
            user: "ada".into(),
            host: "box.example.org".into(),
            port: "2222".into(),
            command: "top".into(),
        };
        assert_eq!(
            format_remote_title("(%u) %H", &target, ""),
            "(ada) box.example.org:2222"
        );
        assert_eq!(format_remote_title("%U%h %c", &target, ""), "ada@box top");
        let address = SshTarget {
            host: "10.0.0.1".into(),
            ..SshTarget::default()
        };
        assert_eq!(format_remote_title("%U%h", &address, ""), "10.0.0.1");
    }

    /// A program that counts what is read of it.
    #[derive(Default)]
    struct Counted {
        ssh: bool,
        names: AtomicUsize,
        arguments: AtomicUsize,
        roots: AtomicUsize,
    }

    impl Programs for Counted {
        fn is_busy(&self) -> bool {
            true
        }

        fn working_directory(&self) -> Option<PathBuf> {
            None
        }

        fn name(&self) -> Option<String> {
            self.names.fetch_add(1, Ordering::Relaxed);
            Some(if self.ssh { "ssh" } else { "vim" }.into())
        }

        fn arguments(&self) -> Option<Vec<String>> {
            self.arguments.fetch_add(1, Ordering::Relaxed);
            Some(if self.ssh {
                vec!["ssh".into(), "box".into()]
            } else {
                vec!["vim".into()]
            })
        }

        fn signal(&self, _signal: Signal) {}

        fn supports(&self, _signal: Signal) -> bool {
            false
        }

        fn set_flow_control(&self, _enabled: bool) {}

        fn is_root(&self) -> bool {
            self.roots.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    #[test]
    fn title_formats_use_konsoles_placeholders() {
        let vim = Counted::default();
        let src = || Some(PathBuf::from("/srv/x/src"));
        assert_eq!(local_title(&vim, &src, "%d : %n", ""), "src : vim");
        assert_eq!(local_title(&vim, &src, "%D %w", "notes"), "/srv/x/src notes");
        assert_eq!(
            local_title(&vim, &src, "%u@%h", ""),
            format!("{}@{}", process::user(), process::host())
        );
        assert_eq!(local_title(&vim, &src, "100%% %x %", ""), "100% %x %");
    }

    #[test]
    fn a_title_reads_only_what_its_format_shows() {
        let looked = AtomicUsize::new(0);
        let directory = || {
            looked.fetch_add(1, Ordering::Relaxed);
            Some(PathBuf::from("/srv/src"))
        };
        // The formats of the settings, for a program that is no ssh: its name once, for the
        // remote format and for %n both, and nothing else.
        let vim = Counted::default();
        assert_eq!(title(&vim, &directory, "%d : %n %n", "(%u) %H", ""), "src : vim vim");
        let read = |counted: &Counted| {
            [&counted.names, &counted.arguments, &counted.roots].map(|count| count.load(Ordering::Relaxed))
        };
        assert_eq!((read(&vim), looked.load(Ordering::Relaxed)), ([1, 0, 0], 1));
        // A format without the directory does not look for it.
        let vim = Counted::default();
        assert_eq!(title(&vim, &directory, "%n %w", "", "notes"), "vim notes");
        assert_eq!((read(&vim), looked.load(Ordering::Relaxed)), ([1, 0, 0], 1));
        // An ssh has its command line read, and its title needs nothing local.
        let ssh = Counted {
            ssh: true,
            ..Counted::default()
        };
        assert_eq!(title(&ssh, &directory, "%d %B", "%h", ""), "box");
        assert_eq!((read(&ssh), looked.load(Ordering::Relaxed)), ([1, 1, 0], 1));
    }

    #[test]
    fn full_directories_shorten_the_home() {
        let home = Some(Path::new("/home/a"));
        assert_eq!(full_directory(Path::new("/home/a"), home), "~");
        assert_eq!(full_directory(Path::new("/home/a/src"), home), "~/src");
        assert_eq!(full_directory(Path::new("/etc"), home), "/etc");
    }

    #[test]
    fn home_is_shortened_to_a_tilde() {
        assert_eq!(short_directory(Path::new("/home/a"), Some(Path::new("/home/a"))), "~");
    }

    #[test]
    fn other_directories_show_their_last_component() {
        assert_eq!(
            short_directory(Path::new("/usr/share/app"), Some(Path::new("/home/a"))),
            "app"
        );
    }

    #[test]
    fn the_root_directory_keeps_its_name() {
        assert_eq!(short_directory(Path::new("/"), None), "/");
    }

    #[test]
    fn random_text_does_no_harm() {
        let target = SshTarget {
            user: "ada".into(),
            host: "box.example.org".into(),
            port: "22".into(),
            command: "ls".into(),
        };
        let samples = ["-p 22 ada@box.example.org ls -l", "%d : %n (%u) %H"];
        for input in crate::fuzz::inputs(&samples) {
            let words: Vec<String> = input.split(' ').map(str::to_string).collect();
            let _ = parse_ssh(&words);
            let _ = local_title(&Shell(false), &|| None, &input, "title");
            let _ = format_remote_title(&input, &target, "title");
            let _ = title(&Ssh(words), &|| None, &input, &input, &input);
        }
    }
}
