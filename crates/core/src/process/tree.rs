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
/// comes last in the list, among those that `counts(parent, child)` takes for a child of
/// the terminal. The shell itself when it runs nothing.
///
/// Windows keeps the parent of a process after the parent ended, and gives its id to new
/// processes soon: an old process can name the shell as its parent. `counts` checks the
/// times they started.
#[must_use]
pub fn foreground(processes: &[Process], shell: u32, counts: impl Fn(u32, &Process) -> bool) -> u32 {
    let mut current = shell;
    // A list with reused process ids could form a loop; no chain is longer than the list.
    for _ in 0..processes.len() {
        let child = processes.iter().rev().find(|process| {
            process.parent == current && process.id != current && !is_console(process) && counts(current, process)
        });
        match child {
            Some(child) => current = child.id,
            None => break,
        }
    }
    current
}

/// The subsystem in the header of a program for Windows, from its first bytes: 2 for a
/// program with windows, 3 for one of the console. `None` for anything else.
#[must_use]
pub fn subsystem(header: &[u8]) -> Option<u16> {
    let bytes = |at: usize, count: usize| header.get(at..at.checked_add(count)?);
    if bytes(0, 2)? != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(bytes(0x3c, 4)?.try_into().ok()?) as usize;
    if bytes(pe, 4)? != b"PE\0\0" {
        return None;
    }
    // The optional header follows the signature and the file header of 20 bytes; its
    // subsystem is at the same place for 32 and 64 bits.
    let at = pe.checked_add(4 + 20 + 68)?;
    Some(u16::from_le_bytes(bytes(at, 2)?.try_into().ok()?))
}

/// The subsystem of programs with windows; see [`subsystem`].
pub const WINDOWS_SUBSYSTEM: u16 = 2;

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
    name(processes, asker(processes, id, is_own)?).filter(|name| !name.is_empty())
}

/// The process behind process `id` for the question about an AI agent: for a process that
/// `is_own` says is `kudari mcp`, the agent which started it.
#[must_use]
pub fn asker(processes: &[Process], id: u32, is_own: impl Fn(u32) -> bool) -> Option<u32> {
    if is_own(id) {
        Some(processes.iter().find(|process| process.id == id)?.parent)
    } else {
        Some(id)
    }
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
        assert_eq!(foreground(&list(), 11, |_, _| true), 16);
        assert_eq!(foreground(&list(), 14, |_, _| true), 14);
        assert_eq!(foreground(&[], 11, |_, _| true), 11);
    }

    #[test]
    fn processes_of_the_console_do_not_count() {
        let list = vec![process(11, 10, "cmd.exe"), process(13, 11, "conhost.exe")];
        assert_eq!(foreground(&list, 11, |_, _| true), 11);
    }

    #[test]
    fn a_process_older_than_its_parent_is_no_child_of_it() {
        // An updater started by a process that ended, whose id the shell 11 has now; the
        // updater runs since before the shell.
        let list = vec![process(20, 11, "updater.exe"), process(11, 10, "pwsh.exe")];
        let started = |id: u32| match id {
            20 => 100,
            11 => 500,
            _ => 900,
        };
        let counts = |parent: u32, child: &Process| started(child.id) >= started(parent);
        assert_eq!(foreground(&list, 11, counts), 11);
        // A program the shell started is its child.
        let list = vec![
            process(20, 11, "updater.exe"),
            process(11, 10, "pwsh.exe"),
            process(30, 11, "git.exe"),
        ];
        assert_eq!(foreground(&list, 11, counts), 30);
    }

    #[test]
    fn a_loop_of_reused_ids_ends() {
        let list = vec![process(1, 2, "a.exe"), process(2, 1, "b.exe"), process(3, 3, "c.exe")];
        assert!([1, 2].contains(&foreground(&list, 1, |_, _| true)));
        assert_eq!(foreground(&list, 3, |_, _| true), 3);
    }

    /// The first bytes of a program for Windows with `subsystem`.
    fn header(subsystem: u16) -> Vec<u8> {
        let mut bytes = vec![0; 0x200];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x80 + 92..0x80 + 94].copy_from_slice(&subsystem.to_le_bytes());
        bytes
    }

    #[test]
    fn the_header_tells_programs_with_windows() {
        assert_eq!(subsystem(&header(2)), Some(WINDOWS_SUBSYSTEM));
        assert_eq!(subsystem(&header(3)), Some(3));
        assert_eq!(subsystem(b"#!/bin/sh"), None);
        assert_eq!(subsystem(&header(2)[..0x90]), None, "cut short");
        let mut far = header(2);
        far[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(subsystem(&far), None);
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
