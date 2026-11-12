// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The shells a new tab can start, and what Kudari adds to them so that they mark their
//! prompt and report their directory (see [`crate::marks`]).
//!
//! On Windows these are the Command Prompt, Windows PowerShell, PowerShell 7 and every WSL
//! distribution. On Linux the list is empty: a terminal starts the login shell, or the
//! `Command` setting, as before. Shells in WSL are Linux shells and set up their marks
//! themselves.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::command::Quoting;

/// What kind of shell it is, which decides how Kudari sets up its marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    CommandPrompt,
    /// Windows PowerShell 5 and PowerShell 7.
    PowerShell,
    Wsl,
}

/// A shell that a new tab can start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shell {
    /// What the `Shell` setting names: `cmd`, `powershell`, `pwsh` or `wsl:` and the name of
    /// the distribution.
    pub id: String,
    pub name: String,
    pub program: PathBuf,
    pub arguments: Vec<String>,
    pub kind: Kind,
}

/// What a new terminal runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub arguments: Vec<String>,
    /// Added to the environment of the shell.
    pub environment: Vec<(String, String)>,
    /// The secret that the shell integration of Kudari writes into its OSC 133 marks, so
    /// that the marks of programs and of the text they show do not count; see
    /// [`crate::marks::Scanner::new`].
    pub key: Option<String>,
    /// How the shell reads quoted paths.
    pub quoting: Quoting,
}

/// What the search for shells asks of the system.
pub trait System {
    fn variable(&self, name: &str) -> Option<String>;
    fn is_file(&self, path: &Path) -> bool;
    /// The output of `wsl.exe --list --quiet`.
    fn distributions(&self, wsl: &Path) -> Option<Vec<u8>>;
}

/// The shells on this computer, found once.
pub fn installed() -> &'static [Shell] {
    static SHELLS: OnceLock<Vec<Shell>> = OnceLock::new();
    SHELLS.get_or_init(|| if cfg!(windows) { discover(&Here) } else { Vec::new() })
}

/// The real system.
struct Here;

impl System for Here {
    fn variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn distributions(&self, wsl: &Path) -> Option<Vec<u8>> {
        let mut command = std::process::Command::new(wsl);
        command.args(["--list", "--quiet"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window flashes up.
            command.creation_flags(0x0800_0000);
        }
        let output = command.stdin(std::process::Stdio::null()).output().ok()?;
        output.status.success().then_some(output.stdout)
    }
}

/// The shells of Windows that `system` has, in the order a menu shows them.
#[must_use]
pub fn discover(system: &dyn System) -> Vec<Shell> {
    let root = PathBuf::from(
        system
            .variable("SystemRoot")
            .unwrap_or_else(|| "C:\\Windows".to_string()),
    );
    let system32 = root.join("System32");
    let mut shells = Vec::new();
    let mut add = |id: &str, name: &str, program: PathBuf, kind: Kind| {
        if system.is_file(&program) {
            shells.push(Shell {
                id: id.to_string(),
                name: name.to_string(),
                program,
                arguments: Vec::new(),
                kind,
            });
        }
    };
    let pwsh = system
        .variable("ProgramFiles")
        .map(|programs| PathBuf::from(programs).join("PowerShell\\7\\pwsh.exe"))
        .filter(|program| system.is_file(program))
        .or_else(|| on_path(system, "pwsh.exe"));
    if let Some(pwsh) = pwsh {
        add("pwsh", "PowerShell 7", pwsh, Kind::PowerShell);
    }
    add(
        "powershell",
        "Windows PowerShell",
        system32.join("WindowsPowerShell\\v1.0\\powershell.exe"),
        Kind::PowerShell,
    );
    let cmd = system
        .variable("ComSpec")
        .map_or_else(|| system32.join("cmd.exe"), PathBuf::from);
    add("cmd", "Command Prompt", cmd, Kind::CommandPrompt);
    let wsl = system32.join("wsl.exe");
    if system.is_file(&wsl) {
        let names = system
            .distributions(&wsl)
            .map(|output| distributions(&output))
            .unwrap_or_default();
        shells.extend(names.into_iter().map(|name| Shell {
            id: format!("wsl:{name}"),
            arguments: vec!["--distribution".to_string(), name.clone()],
            name,
            program: wsl.clone(),
            kind: Kind::Wsl,
        }));
    }
    shells
}

/// A program in one of the directories of `PATH`.
fn on_path(system: &dyn System, program: &str) -> Option<PathBuf> {
    let path = system.variable("PATH")?;
    path.split(';')
        .filter(|directory| !directory.is_empty())
        .map(|directory| PathBuf::from(directory).join(program))
        .find(|candidate| system.is_file(candidate))
}

/// The names in the output of `wsl.exe --list --quiet`, which is UTF-16. The distributions of
/// Docker Desktop are for its engine, not for people.
fn distributions(output: &[u8]) -> Vec<String> {
    let units: Vec<u16> = output
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|line| {
            line.trim_matches(|character: char| {
                character.is_whitespace() || character == '\u{feff}' || character == '\0'
            })
        })
        .filter(|name| !name.is_empty() && !name.starts_with("docker-desktop"))
        .map(str::to_string)
        .collect()
}

/// The shell a new terminal starts without a choice: PowerShell 7, else Windows PowerShell,
/// else the Command Prompt.
#[must_use]
pub fn default_shell(shells: &[Shell]) -> Option<&Shell> {
    ["pwsh", "powershell", "cmd"]
        .iter()
        .find_map(|id| shells.iter().find(|shell| shell.id == *id))
}

/// What a new terminal runs: the `Command` setting if it has one, which Kudari leaves as it
/// is, else the shell chosen for the tab, else the one of the `Shell` setting, else the
/// default shell. `None` leaves the choice to the PTY, which starts the login shell on Unix.
/// `prompt` is the `PROMPT` the Command Prompt would have otherwise.
#[must_use]
pub fn launch(
    command: &str,
    chosen: Option<&str>,
    setting: &str,
    integration: bool,
    shells: &[Shell],
    prompt: Option<&str>,
) -> Option<Launch> {
    if let Some(mut words) = crate::command::split_command(command) {
        let program = crate::command::expand_home(&words.remove(0));
        return Some(Launch {
            quoting: Quoting::of_program(&program),
            program,
            arguments: words,
            environment: Vec::new(),
            key: None,
        });
    }
    let find = |id: &str| shells.iter().find(|shell| shell.id == id);
    let shell = chosen
        .and_then(find)
        .or_else(|| find(setting))
        .or_else(|| default_shell(shells))?;
    let mut launch = Launch {
        program: shell.program.to_string_lossy().into_owned(),
        arguments: shell.arguments.clone(),
        environment: Vec::new(),
        key: None,
        quoting: match shell.kind {
            Kind::CommandPrompt => Quoting::CommandPrompt,
            Kind::PowerShell => Quoting::PowerShell,
            Kind::Wsl => Quoting::Posix,
        },
    };
    let key = if integration { new_key() } else { None };
    if let Some(key) = key {
        match shell.kind {
            Kind::CommandPrompt => {
                launch
                    .environment
                    .push(("PROMPT".to_string(), command_prompt(prompt.unwrap_or("$P$G"), &key)));
                launch.key = Some(key);
            }
            Kind::PowerShell => {
                launch.arguments.extend(
                    ["-NoLogo", "-NoExit", "-EncodedCommand"]
                        .map(str::to_string)
                        .into_iter()
                        .chain(std::iter::once(encoded_command(POWERSHELL))),
                );
                launch.environment.push((KEY_VARIABLE.to_string(), key.clone()));
                launch.key = Some(key);
            }
            Kind::Wsl => {}
        }
    }
    Some(launch)
}

/// Where PowerShell finds the key of its marks; the script takes it out of the environment,
/// so that the programs it starts do not see it.
const KEY_VARIABLE: &str = "KUDARI_MARK";

/// A new secret for the marks of a terminal: 128 random bits in hex. Without randomness,
/// the shell gets no marks, as without shell integration.
fn new_key() -> Option<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).ok()?;
    let mut key = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(key, "{byte:02x}");
    }
    Some(key)
}

/// The `PROMPT` of the Command Prompt around `prompt`, with the marks that Windows Terminal
/// documents: the end of the last command, the start of the prompt, the directory, and the
/// start of the command line, each signed with `key`. The Command Prompt cannot mark the
/// start of a command.
fn command_prompt(prompt: &str, key: &str) -> String {
    format!("$e]133;D;kudari={key}$e\\$e]133;A;kudari={key}$e\\$e]9;9;$P$e\\{prompt}$e]133;B;kudari={key}$e\\")
}

/// Wraps the prompt of the profile in the marks, and marks the start of a command when
/// Return still accepts the line. Every mark carries the key, which the script takes out of
/// the environment. In the vi mode of `PSReadLine`, keys of the command mode act instead of
/// going into the line, so the start of the command line goes unmarked there. Written for
/// Windows PowerShell 5 as well, which knows no `` `e ``.
const POWERSHELL: &str = r#"$global:KudariMark = "kudari=$env:KUDARI_MARK"
Remove-Item Env:KUDARI_MARK -ErrorAction Ignore
$global:KudariPrompt = $function:prompt
function global:prompt {
    $ok = $?
    $e = [char]27
    $key = $global:KudariMark
    $location = $executionContext.SessionState.Path.CurrentLocation
    $marks = "$e]133;D;$(if ($ok) { 0 } else { 1 });$key$e\$e]133;A;$key$e\"
    if ($location.Provider.Name -eq 'FileSystem') {
        $marks += "$e]9;9;`"$($location.ProviderPath)`"$e\"
    }
    $command = "$e]133;B;$key$e\"
    if ((Get-Module PSReadLine) -and (Get-PSReadLineOption).EditMode -eq 'Vi') {
        $command = ''
    }
    "$marks$(& $global:KudariPrompt)$command"
}
if ((Get-Module PSReadLine) -and ((Get-PSReadLineKeyHandler -Chord Enter).Function -eq 'AcceptLine')) {
    Set-PSReadLineKeyHandler -Chord Enter -ScriptBlock {
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        [Console]::Write("$([char]27)]133;C;$($global:KudariMark)$([char]27)\")
    }
}
"#;

/// A script for `-EncodedCommand`: UTF-16 in Base64, so no quoting can break it.
fn encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or_default());
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for (position, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if position <= chunk.len() {
                let index = ((group >> shift) & 0x3f) as usize;
                text.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// A Windows with the given files and variables.
    struct Fake {
        files: BTreeSet<String>,
        variables: Vec<(&'static str, &'static str)>,
        wsl: Option<Vec<u8>>,
    }

    impl System for Fake {
        fn variable(&self, name: &str) -> Option<String> {
            self.variables
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }

        /// Paths joined on Linux have slashes; Windows reads both alike.
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(&windows_path(path))
        }

        fn distributions(&self, _wsl: &Path) -> Option<Vec<u8>> {
            self.wsl.clone()
        }
    }

    fn windows_path(path: &Path) -> String {
        path.to_string_lossy().replace('/', "\\")
    }

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn windows(files: &[&str]) -> Fake {
        Fake {
            files: files.iter().map(|file| (*file).to_string()).collect(),
            variables: vec![("SystemRoot", "C:\\Windows"), ("ProgramFiles", "C:\\Program Files")],
            wsl: Some(utf16("\u{feff}Ubuntu\r\ndocker-desktop\r\nDebian\r\n\0")),
        }
    }

    const ALL: [&str; 4] = [
        "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        "C:\\Windows\\System32\\cmd.exe",
        "C:\\Windows\\System32\\wsl.exe",
    ];

    fn ids(shells: &[Shell]) -> Vec<&str> {
        shells.iter().map(|shell| shell.id.as_str()).collect()
    }

    #[test]
    fn every_shell_of_windows_is_found() {
        let shells = discover(&windows(&ALL));
        assert_eq!(ids(&shells), ["pwsh", "powershell", "cmd", "wsl:Ubuntu", "wsl:Debian"]);
        let ubuntu = shells.iter().find(|shell| shell.id == "wsl:Ubuntu").unwrap();
        assert_eq!(ubuntu.arguments, ["--distribution", "Ubuntu"]);
        assert_eq!(ubuntu.name, "Ubuntu");
        assert_eq!(default_shell(&shells).unwrap().id, "pwsh");
    }

    #[test]
    fn missing_shells_are_left_out() {
        let shells = discover(&windows(&["C:\\Windows\\System32\\cmd.exe"]));
        assert_eq!(ids(&shells), ["cmd"]);
        assert_eq!(default_shell(&shells).unwrap().id, "cmd");
        assert!(discover(&windows(&[])).is_empty());
        assert_eq!(default_shell(&[]), None);
    }

    #[test]
    fn powershell_7_is_also_found_on_the_path_and_cmd_by_comspec() {
        let mut system = windows(&["D:\\Tools\\pwsh.exe", "D:\\cmd.exe"]);
        system.variables.push(("PATH", "C:\\nothing;;D:\\Tools"));
        system.variables.push(("ComSpec", "D:\\cmd.exe"));
        let shells = discover(&system);
        assert_eq!(ids(&shells), ["pwsh", "cmd"]);
        assert_eq!(windows_path(&shells.first().unwrap().program), "D:\\Tools\\pwsh.exe");
    }

    #[test]
    fn a_broken_wsl_lists_no_distributions() {
        let mut system = windows(&["C:\\Windows\\System32\\wsl.exe"]);
        system.wsl = None;
        assert!(discover(&system).is_empty());
        assert!(distributions(&[0x41]).is_empty());
    }

    #[test]
    fn the_command_setting_comes_first_and_stays_as_it_is() {
        let shells = discover(&windows(&ALL));
        let launch = launch("vim -n", Some("cmd"), "pwsh", true, &shells, None).unwrap();
        assert_eq!(launch.program, "vim");
        assert_eq!(launch.arguments, ["-n"]);
        assert!(launch.environment.is_empty());
    }

    #[test]
    fn each_shell_gets_its_quoting() {
        let shells = discover(&windows(&ALL));
        let quoting = |chosen| launch("", Some(chosen), "", false, &shells, None).unwrap().quoting;
        assert_eq!(quoting("cmd"), Quoting::CommandPrompt);
        assert_eq!(quoting("powershell"), Quoting::PowerShell);
        assert_eq!(quoting("pwsh"), Quoting::PowerShell);
        assert_eq!(quoting("wsl:Debian"), Quoting::Posix);
        let command = |command| launch(command, None, "", false, &shells, None).unwrap().quoting;
        assert_eq!(command("C:/Windows/System32/cmd.exe /k"), Quoting::CommandPrompt);
        assert_eq!(command("zsh -l"), Quoting::Posix);
    }

    #[test]
    fn a_tab_starts_its_shell_then_the_setting_then_the_default() {
        let shells = discover(&windows(&ALL));
        let program = |chosen, setting| launch("", chosen, setting, false, &shells, None).unwrap().program;
        assert!(program(Some("cmd"), "powershell").ends_with("cmd.exe"));
        assert!(program(None, "powershell").ends_with("powershell.exe"));
        assert!(program(Some("gone"), "gone").ends_with("pwsh.exe"));
        assert_eq!(launch("", None, "", true, &[], None), None);
    }

    #[test]
    fn the_command_prompt_marks_its_prompt() {
        let shells = discover(&windows(&ALL));
        let launch = launch("", Some("cmd"), "", true, &shells, Some("$N$G")).unwrap();
        let key = launch.key.clone().unwrap();
        assert_eq!(key.len(), 32);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let signed = format!(";kudari={key}$e\\");
        assert_eq!(
            launch.environment,
            [(
                "PROMPT".to_string(),
                format!("$e]133;D{signed}$e]133;A{signed}$e]9;9;$P$e\\$N$G$e]133;B{signed}")
            )]
        );
        let plain = super::launch("", Some("cmd"), "", true, &shells, None).unwrap();
        assert!(plain.environment.first().unwrap().1.contains("$P$G$e]133;B"));
        assert_ne!(plain.key, launch.key, "every terminal has a key of its own");
        let off = super::launch("", Some("cmd"), "", false, &shells, None).unwrap();
        assert!(off.environment.is_empty());
        assert_eq!(off.key, None);
    }

    #[test]
    fn powershell_gets_its_marks_as_an_encoded_command() {
        let shells = discover(&windows(&ALL));
        for id in ["pwsh", "powershell"] {
            let launch = launch("", Some(id), "", true, &shells, None).unwrap();
            assert_eq!(
                launch.arguments.get(..3).unwrap(),
                ["-NoLogo", "-NoExit", "-EncodedCommand"]
            );
            assert_eq!(launch.arguments.get(3), Some(&encoded_command(POWERSHELL)));
            let key = launch.key.unwrap();
            assert_eq!(launch.environment, [(KEY_VARIABLE.to_string(), key)]);
        }
        let wsl = launch("", Some("wsl:Debian"), "", true, &shells, None).unwrap();
        assert_eq!(wsl.arguments, ["--distribution", "Debian"]);
        assert_eq!(wsl.key, None);
    }

    #[test]
    fn base64_follows_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // `dir` as PowerShell expects it.
        assert_eq!(encoded_command("dir"), "ZABpAHIA");
    }

    #[test]
    fn no_shells_are_found_off_windows() {
        if !cfg!(windows) {
            assert!(installed().is_empty());
        }
    }
}
