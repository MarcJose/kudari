// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Command lines as the settings hold them, and paths as a shell reads them.

/// Replaces a leading `~` with the home directory.
#[must_use]
pub fn expand_home(path: &str) -> String {
    match (path.strip_prefix('~'), crate::process::home()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', std::path::MAIN_SEPARATOR]) => {
            format!("{}{rest}", home.display())
        }
        _ => path.to_string(),
    }
}

/// Splits a command line like a shell does for quotes, and on Unix for backslashes too. No
/// variable or glob expansion: the words reach the program unchanged. `None` for an empty
/// command or an unterminated quote.
#[must_use]
pub fn split_command(command: &str) -> Option<Vec<String>> {
    split(command, !cfg!(windows))
}

/// Splits as a POSIX shell with `escapes`, else as Windows, where `\` separates the parts of a
/// path and only quotes group.
fn split(command: &str, escapes: bool) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote = None;
    let mut characters = command.chars();
    while let Some(character) = characters.next() {
        match (quote, character) {
            (Some(open), _) if open == character => quote = None,
            (None, '\'' | '"') => {
                quote = Some(character);
                word.get_or_insert_default();
            }
            (None | Some('"'), '\\') if escapes => word.get_or_insert_default().extend(characters.next()),
            (None, _) if character.is_whitespace() => words.extend(word.take()),
            _ => word.get_or_insert_default().push(character),
        }
    }
    words.extend(word);
    (quote.is_none() && !words.is_empty()).then_some(words)
}

/// How a shell reads a quoted word, which differs between the shells of Unix and Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quoting {
    /// sh, bash, zsh, fish and the like, also in WSL: single quotes, with `'` and `\\` outside
    /// of them, as fish reads backslashes within them.
    #[default]
    Posix,
    /// cmd.exe: double quotes, which keep `&`, `|`, `<`, `>` and `^` from it.
    CommandPrompt,
    /// Windows PowerShell and PowerShell 7: single quotes, also the typographic ones.
    PowerShell,
}

impl Quoting {
    /// The quoting of the shell that runs when none is chosen: PowerShell on Windows, as
    /// with `alacritty_terminal`, else the login shell.
    #[must_use]
    pub fn system() -> Self {
        if cfg!(windows) { Self::PowerShell } else { Self::Posix }
    }

    /// The quoting of a program by its file name, for a command of the settings.
    #[must_use]
    pub fn of_program(program: &str) -> Self {
        let name = program.rsplit(['/', '\\']).next().unwrap_or_default().to_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        match name {
            "cmd" => Self::CommandPrompt,
            "powershell" | "pwsh" => Self::PowerShell,
            _ => Self::Posix,
        }
    }
}

/// The command that changes into the directory `quoted`, already quoted for the shell:
/// cmd.exe changes the drive only with `/d`.
#[must_use]
pub fn change_directory(quoted: &str, quoting: Quoting) -> String {
    match quoting {
        Quoting::CommandPrompt => format!("cd /d {quoted}"),
        Quoting::Posix | Quoting::PowerShell => format!("cd {quoted}"),
    }
}

/// Quotes a path so that the shell reads it as one word and runs nothing in it. `None` for a
/// path the shell cannot take safely: control characters would act as keys, and cmd.exe
/// expands `%` even within quotes and cannot quote `"`.
#[must_use]
pub fn quote_path(path: &str, quoting: Quoting) -> Option<String> {
    if path.chars().any(char::is_control) {
        return None;
    }
    match quoting {
        Quoting::Posix => {
            let mut quoted = String::from("'");
            for character in path.chars() {
                match character {
                    '\'' => quoted.push_str("'\\''"),
                    '\\' => quoted.push_str("'\\\\'"),
                    _ => quoted.push(character),
                }
            }
            quoted.push('\'');
            Some(quoted)
        }
        Quoting::CommandPrompt => (!path.contains(['"', '%'])).then(|| format!("\"{path}\"")),
        Quoting::PowerShell => {
            // Within single quotes, PowerShell takes any of its single quotes, doubled, as one.
            let mut quoted = String::from("'");
            for character in path.chars() {
                if matches!(character, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                    quoted.push(character);
                }
                quoted.push(character);
            }
            quoted.push('\'');
            Some(quoted)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_keeps_its_backslashes() {
        let words = |command: &str| split(command, false).unwrap();
        assert_eq!(
            words(r"C:\Windows\System32\cmd.exe /k"),
            [r"C:\Windows\System32\cmd.exe", "/k"]
        );
        assert_eq!(
            words(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#),
            [r"C:\Program Files\PowerShell\7\pwsh.exe", "-NoLogo"]
        );
    }

    #[test]
    fn commands_are_split_like_in_a_shell() {
        let words = |command: &str| split(command, true).unwrap();
        assert_eq!(words("/bin/zsh -l"), ["/bin/zsh", "-l"]);
        assert_eq!(words("sh -c 'echo a  b; exec sh'"), ["sh", "-c", "echo a  b; exec sh"]);
        assert_eq!(words(r#"tmux new "a b" c\ d ''"#), ["tmux", "new", "a b", "c d", ""]);
        assert_eq!(split("   ", true), None);
        assert_eq!(split("sh -c 'open", true), None);
    }

    #[test]
    fn paths_are_quoted_for_each_shell() {
        let posix = |path| quote_path(path, Quoting::Posix);
        assert_eq!(posix("/tmp/a b").as_deref(), Some("'/tmp/a b'"));
        assert_eq!(posix("it's").as_deref(), Some("'it'\\''s'"));
        assert_eq!(posix("$(rm -rf ~)`x`;&|").as_deref(), Some("'$(rm -rf ~)`x`;&|'"));
        // fish ends a quote at \' and keeps \\ within it; outside, both shells read \\ alike.
        assert_eq!(posix(r"x\").as_deref(), Some(r"'x'\\''"));
        assert_eq!(posix(r"a\b").as_deref(), Some(r"'a'\\'b'"));

        let cmd = |path| quote_path(path, Quoting::CommandPrompt);
        assert_eq!(cmd(r"C:\Tom & Jerry.txt").as_deref(), Some(r#""C:\Tom & Jerry.txt""#));
        assert_eq!(cmd(r"C:\a|b^c<d>e").as_deref(), Some(r#""C:\a|b^c<d>e""#));
        assert_eq!(cmd(r"C:\%PATH%.txt"), None, "cmd expands it within quotes");
        assert_eq!(cmd(r#"C:\a"b"#), None);

        let powershell = |path| quote_path(path, Quoting::PowerShell);
        assert_eq!(
            powershell(r"C:\$(calc) & x.txt").as_deref(),
            Some(r"'C:\$(calc) & x.txt'")
        );
        assert_eq!(powershell("it's").as_deref(), Some("'it''s'"));
        assert_eq!(powershell("a\u{2019}b").as_deref(), Some("'a\u{2019}\u{2019}b'"));

        for quoting in [Quoting::Posix, Quoting::CommandPrompt, Quoting::PowerShell] {
            assert_eq!(quote_path("a\nb", quoting), None, "a new line would act as Return");
            assert_eq!(quote_path("a\u{1b}[201~", quoting), None);
        }
    }

    #[test]
    fn cmd_changes_the_drive_as_well() {
        assert_eq!(
            change_directory(r#""D:\src""#, Quoting::CommandPrompt),
            r#"cd /d "D:\src""#
        );
        assert_eq!(change_directory("'/srv'", Quoting::Posix), "cd '/srv'");
        assert_eq!(change_directory(r"'D:\src'", Quoting::PowerShell), r"cd 'D:\src'");
    }

    #[test]
    fn the_program_tells_the_quoting() {
        assert_eq!(
            Quoting::of_program(r"C:\Windows\System32\cmd.exe"),
            Quoting::CommandPrompt
        );
        assert_eq!(Quoting::of_program("CMD.EXE"), Quoting::CommandPrompt);
        assert_eq!(
            Quoting::of_program(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            Quoting::PowerShell
        );
        assert_eq!(Quoting::of_program("powershell"), Quoting::PowerShell);
        assert_eq!(Quoting::of_program("/bin/zsh"), Quoting::Posix);
        assert_eq!(Quoting::of_program("wsl.exe"), Quoting::Posix);
    }

    #[test]
    fn only_a_leading_tilde_is_the_home_directory() {
        let Some(home) = crate::process::home() else {
            return;
        };
        let home = home.display().to_string();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/bin"), format!("{home}/bin"));
        assert_eq!(expand_home("~ada/bin"), "~ada/bin");
        assert_eq!(expand_home("/bin/~"), "/bin/~");
    }

    #[test]
    fn random_commands_do_no_harm() {
        for input in crate::fuzz::inputs(&["sh -c 'echo a'", "~/bin/x \\ \"y\""]) {
            let _ = split_command(&input);
            let _ = expand_home(&input);
            for quoting in [Quoting::Posix, Quoting::CommandPrompt, Quoting::PowerShell] {
                let _ = quote_path(&input, quoting);
            }
        }
    }
}
