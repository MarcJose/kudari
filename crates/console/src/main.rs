// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! `kudari.com`, beside `kudari.exe` of Kudari for Windows. That one is a program without a
//! console, so that starting it opens none, and shells do not wait for such a program: the
//! output of `kudari --help` came after the next prompt, and the exit code got lost. Shells
//! look for `.com` before `.exe`, so `kudari` in a shell starts this one, a program of the
//! console. For the commands of the command line, it runs `kudari.exe` with them, on its own
//! standard input and output, and waits for it; anything else starts the window, which
//! `kudari.exe` gets on its own while this one returns at once, as the window does not
//! belong to the shell.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// `kudari.exe` beside this program; `kudari-windows.exe` as cargo builds it.
fn program() -> Option<PathBuf> {
    let directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
    ["kudari.exe", "kudari-windows.exe"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}

/// How the window is started: without the console, which it would keep otherwise.
fn on_its_own(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(DETACHED_PROCESS);
    }
    command
}

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let Some(program) = program() else {
        eprintln!("kudari: kudari.exe is missing beside kudari.com");
        return ExitCode::FAILURE;
    };
    let mut command = Command::new(program);
    command.args(&arguments);
    let waits = arguments
        .first()
        .and_then(|first| first.to_str())
        .is_some_and(|first| matches!(first, "mcp" | "--version" | "-v" | "--help" | "-h"));
    if !waits {
        return match on_its_own(&mut command).spawn() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("kudari: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match command.status() {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("kudari: {error}");
            ExitCode::FAILURE
        }
    }
}
