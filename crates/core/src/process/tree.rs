// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The programs of a terminal from a list of all processes, as Windows gives it: there is no
//! foreground process group, so the program in the foreground is the newest descendant of
//! the shell.

/// One entry of the list of processes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub id: u32,
    pub parent: u32,
    /// The filename of the program, such as `pwsh.exe`.
    pub name: String,
}

/// Processes that belong to the console rather than to the programs in it.
fn is_console(process: &Process) -> bool {
    ["conhost.exe", "OpenConsole.exe"]
        .iter()
        .any(|name| process.name.eq_ignore_ascii_case(name))
}

/// The program in the foreground: from the shell down, always to the newest child, which
/// comes last in the list. The shell itself when it runs nothing.
#[must_use]
pub fn foreground(processes: &[Process], shell: u32) -> u32 {
    let mut current = shell;
    // A list with reused process ids could form a loop; no chain is longer than the list.
    for _ in 0..processes.len() {
        let child = processes
            .iter()
            .rev()
            .find(|process| process.parent == current && process.id != current && !is_console(process));
        match child {
            Some(child) => current = child.id,
            None => break,
        }
    }
    current
}

/// The name of a program without `.exe`, as Linux shows it.
#[must_use]
pub fn name(processes: &[Process], id: u32) -> Option<String> {
    let process = processes.iter().find(|process| process.id == id)?;
    let name = &process.name;
    let stem = name
        .len()
        .checked_sub(4)
        .filter(|&end| {
            name.get(end..)
                .is_some_and(|extension| extension.eq_ignore_ascii_case(".exe"))
        })
        .and_then(|end| name.get(..end))
        .unwrap_or(name);
    Some(stem.to_string())
}

/// The program behind process `id` for the question about an AI agent, without `.exe`; for
/// a process that `is_own` says is `kudari mcp`, the agent which started it.
#[must_use]
pub fn program(processes: &[Process], id: u32, is_own: impl Fn(u32) -> bool) -> Option<String> {
    let id = if is_own(id) {
        processes.iter().find(|process| process.id == id)?.parent
    } else {
        id
    };
    name(processes, id).filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(id: u32, parent: u32, name: &str) -> Process {
        Process {
            id,
            parent,
            name: name.to_string(),
        }
    }

    fn list() -> Vec<Process> {
        vec![
            process(4, 0, "System"),
            process(10, 4, "kudari.exe"),
            process(11, 10, "pwsh.exe"),
            process(12, 10, "conhost.exe"),
            process(13, 11, "OpenConsole.exe"),
            process(14, 11, "git.exe"),
            process(15, 11, "ssh.EXE"),
            process(16, 15, "ssh-agent.exe"),
        ]
    }

    #[test]
    fn the_program_of_an_agent_is_the_parent_of_kudari_mcp() {
        assert_eq!(program(&list(), 14, |_| false).as_deref(), Some("git"));
        assert_eq!(program(&list(), 11, |id| id == 11).as_deref(), Some("kudari"));
        assert_eq!(program(&list(), 99, |_| false), None);
        assert_eq!(program(&list(), 99, |_| true), None);
    }

    #[test]
    fn the_newest_descendant_is_in_the_foreground() {
        assert_eq!(foreground(&list(), 11), 16);
        assert_eq!(foreground(&list(), 14), 14);
        assert_eq!(foreground(&[], 11), 11);
    }

    #[test]
    fn processes_of_the_console_do_not_count() {
        let list = vec![process(11, 10, "cmd.exe"), process(13, 11, "conhost.exe")];
        assert_eq!(foreground(&list, 11), 11);
    }

    #[test]
    fn a_loop_of_reused_ids_ends() {
        let list = vec![process(1, 2, "a.exe"), process(2, 1, "b.exe"), process(3, 3, "c.exe")];
        assert!([1, 2].contains(&foreground(&list, 1)));
        assert_eq!(foreground(&list, 3), 3);
    }

    #[test]
    fn names_lose_their_extension() {
        assert_eq!(name(&list(), 11).as_deref(), Some("pwsh"));
        assert_eq!(name(&list(), 15).as_deref(), Some("ssh"));
        assert_eq!(name(&list(), 4).as_deref(), Some("System"));
        assert_eq!(name(&[process(1, 0, ".exe")], 1).as_deref(), Some(""));
        assert_eq!(name(&[process(1, 0, "é")], 1).as_deref(), Some("é"));
        assert_eq!(name(&list(), 99), None);
    }
}
