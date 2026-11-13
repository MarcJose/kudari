// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The titles of terminals, with the placeholders of Konsole, so that formats carry over.

use std::path::{Path, PathBuf};

use crate::process::{self, Programs};

/// The title of a terminal: `remote_format` while `ssh` runs in the foreground, unless it
/// is empty, else `format`. `directory` is the working directory, as far as it is known;
/// `window_title` is the title the program set.
#[must_use]
pub fn title(
    programs: &dyn Programs,
    directory: Option<PathBuf>,
    format: &str,
    remote_format: &str,
    window_title: &str,
) -> String {
    if !remote_format.is_empty()
        && let Some(target) = ssh_target(programs)
    {
        return format_remote_title(remote_format, &target, window_title);
    }
    local_title(programs, directory, format, window_title)
}

/// The text of a format string; see [`format_title`].
#[must_use]
pub fn local_title(programs: &dyn Programs, directory: Option<PathBuf>, format: &str, window_title: &str) -> String {
    let home = process::home();
    let values = TitleValues {
        directory: directory
            .as_ref()
            .map_or_else(String::new, |path| short_directory(path, home.as_deref())),
        full_directory: directory
            .as_ref()
            .map_or_else(String::new, |path| full_directory(path, home.as_deref())),
        process: programs.name().unwrap_or_default(),
        user: process::user(),
        host: process::host(),
        window_title: window_title.to_string(),
    };
    format_title(format, &values)
}

/// The target of an `ssh` running in the foreground.
fn ssh_target(programs: &dyn Programs) -> Option<SshTarget> {
    let arguments = programs.arguments()?;
    let program = Path::new(arguments.first()?).file_name()?.to_str()?;
    matches!(program, "ssh" | "ssh.exe").then(|| parse_ssh(arguments.get(1..)?))?
}

/// What a title format can contain.
#[derive(Debug, Default)]
pub struct TitleValues {
    pub directory: String,
    pub full_directory: String,
    pub process: String,
    pub user: String,
    pub host: String,
    pub window_title: String,
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
/// `%d` directory (short), `%D` directory (full), `%n` program, `%u` user, `%h` host and
/// `%w` the title set by the program.
#[must_use]
pub fn format_title(format: &str, values: &TitleValues) -> String {
    fill_in(format, |letter| {
        let value = match letter {
            'd' => &values.directory,
            'D' => &values.full_directory,
            'n' => &values.process,
            'u' => &values.user,
            'h' => &values.host,
            'w' => &values.window_title,
            _ => return None,
        };
        Some(value.clone())
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

/// Reads the arguments of `ssh`, with the option letters of its manual.
fn parse_ssh(arguments: &[String]) -> Option<SshTarget> {
    const WITH_VALUE: &str = "bcDeEFIiJLlmOopQRSWw";
    let mut target = SshTarget::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            let (user, host) = word
                .rsplit_once('@')
                .map_or(("", word.as_str()), |(user, host)| (user, host));
            if !user.is_empty() {
                target.user = user.to_string();
            }
            target.host = host.to_string();
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

    #[test]
    fn ssh_gets_the_remote_title() {
        assert_eq!(
            title(&ssh("/usr/bin/ssh -p 2222 ada@box"), None, "%d", "%U%h", ""),
            "ada@box"
        );
        assert_eq!(title(&ssh("ssh.exe box uptime"), None, "%d", "%h %c", ""), "box uptime");
        assert_eq!(
            title(&ssh("ssh box"), Some(PathBuf::from("/srv/src")), "%d : %n", "", ""),
            "src : ssh"
        );
        assert_eq!(title(&ssh("sshd -D"), None, "%n %w", "%h", "notes"), "ssh notes");
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

    #[test]
    fn title_formats_use_konsoles_placeholders() {
        let values = TitleValues {
            directory: "src".into(),
            full_directory: "~/x/src".into(),
            process: "vim".into(),
            user: "ada".into(),
            host: "box".into(),
            window_title: "notes".into(),
        };
        assert_eq!(format_title("%d : %n", &values), "src : vim");
        assert_eq!(format_title("%u@%h:%D %w", &values), "ada@box:~/x/src notes");
        assert_eq!(format_title("100%% %x %", &values), "100% %x %");
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
        let values = TitleValues::default();
        let samples = ["-p 22 ada@box.example.org ls -l", "%d : %n (%u) %H"];
        for input in crate::fuzz::inputs(&samples) {
            let words: Vec<String> = input.split(' ').map(str::to_string).collect();
            let _ = parse_ssh(&words);
            let _ = format_title(&input, &values);
            let _ = format_remote_title(&input, &target, "title");
            let _ = title(&Ssh(words), None, &input, &input, &input);
        }
    }
}
