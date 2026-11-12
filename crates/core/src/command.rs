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

/// Splits a command line like a shell does for quotes and backslashes. No variable or
/// glob expansion: the words reach the program unchanged. `None` for an empty command
/// or an unterminated quote.
#[must_use]
pub fn split_command(command: &str) -> Option<Vec<String>> {
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
            (None | Some('"'), '\\') => word.get_or_insert_default().extend(characters.next()),
            (None, _) if character.is_whitespace() => words.extend(word.take()),
            _ => word.get_or_insert_default().push(character),
        }
    }
    words.extend(word);
    (quote.is_none() && !words.is_empty()).then_some(words)
}

/// Quotes a path for the shell.
#[must_use]
pub fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_split_like_in_a_shell() {
        let words = |command: &str| split_command(command).unwrap();
        assert_eq!(words("/bin/zsh -l"), ["/bin/zsh", "-l"]);
        assert_eq!(words("sh -c 'echo a  b; exec sh'"), ["sh", "-c", "echo a  b; exec sh"]);
        assert_eq!(words(r#"tmux new "a b" c\ d ''"#), ["tmux", "new", "a b", "c d", ""]);
        assert_eq!(split_command("   "), None);
        assert_eq!(split_command("sh -c 'open"), None);
    }

    #[test]
    fn paths_are_quoted_for_the_shell() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
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
            let _ = shell_quote(&input);
        }
    }
}
