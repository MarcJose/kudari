// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What a shell tells about itself in its output: the working directory through OSC 7 or,
//! as Windows Terminal reads it, OSC 9;9, and the parts of its prompt through OSC 133. Linux
//! knows the directory from `/proc` as well; Windows has nothing but these marks.
//!
//! [`Tee`] sits between the PTY and the emulator and reads the marks from everything that
//! passes; the emulator still gets every byte.
//!
//! Any program can print these sequences, a file shown with `cat` as well, and a new tab
//! starts in the directory; the Command Prompt even runs programs from there before those
//! on the path. So a directory counts only while the shell writes its prompt, between the
//! OSC 133 marks `A` and `B`, and only while the operating system has the shell itself in
//! the foreground; a shell without these marks reports no directory. It has to be absolute,
//! and it must not be a share on the network: on Windows, a shell started there would log
//! in to the server with the credentials of the user.
//!
//! Where an agent may type is a matter of more trust: the marks of the shell integration of
//! Kudari carry a key of their own terminal, and in a terminal with a key, marks without it
//! do not count at all. Without a key, the marks never say that the user types a command.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};

/// The longest sequence that is read; longer ones are skipped.
const MAX_SEQUENCE: usize = 4096;

/// Where the shell is in its prompt, after OSC 133.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Prompt {
    /// The shell sends no marks, or none yet.
    #[default]
    Unknown,
    /// `A`: the prompt is being written.
    Prompt,
    /// `B`: the user types a command.
    Command,
    /// `C`: the command runs.
    Running,
    /// `D`: the command ended.
    Finished,
}

/// What the marks said last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub directory: Option<PathBuf>,
    pub prompt: Prompt,
    /// The shell marks where its commands start, so that [`Prompt::Command`] ends when one
    /// runs, and signs its marks with the key of the terminal. The Command Prompt cannot
    /// mark a command, and stays at `Command` while one runs.
    pub commands_marked: bool,
}

impl Marks {
    /// Whether the user types a command at the prompt of the shell, as far as the marks can
    /// tell: only signed ones of a shell that also marks where its commands start.
    #[must_use]
    pub fn at_command_line(&self) -> bool {
        self.prompt == Prompt::Command && self.commands_marked
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    /// After ESC.
    Escape,
    /// Inside OSC.
    Osc,
    /// After ESC inside OSC, which starts the terminator ESC `\`.
    OscEscape,
}

/// Finds the marks in output that comes in pieces.
#[derive(Debug)]
pub struct Scanner {
    /// The key that signs the OSC 133 marks of the shell, as `kudari=` and the key.
    key: Option<String>,
    state: State,
    sequence: Vec<u8>,
    /// The sequence grew too long and is skipped up to its end.
    overflow: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Scanner {
    /// A scanner for a shell that signs its OSC 133 marks with `key`, the
    /// [`crate::shells::Launch::key`]: marks without it do not count.
    #[must_use]
    pub fn new(key: Option<&str>) -> Self {
        Self {
            key: key.map(|key| format!("kudari={key}")),
            state: State::Ground,
            sequence: Vec::new(),
            overflow: false,
        }
    }

    /// Reads `bytes` and updates `marks` with every sequence that ends in them.
    /// `at_shell` says whether the shell itself is in the foreground; it is asked only for
    /// a directory.
    pub fn scan(&mut self, bytes: &[u8], marks: &Mutex<Marks>, at_shell: &dyn Fn() -> bool) {
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.state == State::Ground {
                // Most output has no escape at all.
                let Some(position) = rest.iter().position(|&byte| byte == 0x1b) else {
                    return;
                };
                self.state = State::Escape;
                rest = rest.get(position + 1..).unwrap_or_default();
                continue;
            }
            let Some((&byte, after)) = rest.split_first() else {
                return;
            };
            rest = after;
            self.state = match (self.state, byte) {
                // ESC `]` starts a sequence, also inside one.
                (State::Escape | State::OscEscape, b']') => {
                    self.sequence.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::Escape, 0x1b) => State::Escape,
                (State::Osc, 0x07) | (State::OscEscape, b'\\') => {
                    if !self.overflow {
                        apply(&self.sequence, self.key.as_deref(), marks, at_shell);
                    }
                    State::Ground
                }
                (State::Osc | State::OscEscape, 0x1b) => State::OscEscape,
                (State::Osc, _) => {
                    if self.sequence.len() < MAX_SEQUENCE {
                        self.sequence.push(byte);
                    } else {
                        self.overflow = true;
                    }
                    State::Osc
                }
                // Anything else ends the sequence without a mark.
                _ => State::Ground,
            };
        }
    }
}

/// Takes in one OSC sequence, without its introducer and terminator. `signature` is the
/// field that signs the OSC 133 marks of the shell.
fn apply(sequence: &[u8], signature: Option<&str>, marks: &Mutex<Marks>, at_shell: &dyn Fn() -> bool) {
    let Ok(text) = std::str::from_utf8(sequence) else {
        return;
    };
    let mut marks = marks.lock().unwrap_or_else(PoisonError::into_inner);
    let at_prompt = || marks.prompt == Prompt::Prompt && at_shell();
    if let Some(url) = text.strip_prefix("7;") {
        if let Some(directory) = local_directory(url).and_then(acceptable).filter(|_| at_prompt()) {
            marks.directory = Some(directory);
        }
    } else if let Some(path) = text.strip_prefix("9;9;") {
        let path = PathBuf::from(path.trim_matches('"'));
        if let Some(directory) = acceptable(path).filter(|_| at_prompt()) {
            marks.directory = Some(directory);
        }
    } else if let Some(mark) = text.strip_prefix("133;") {
        let mut fields = mark.split(';');
        let kind = fields.next().unwrap_or_default();
        let signed = signature.is_some_and(|signature| fields.any(|field| field == signature));
        if signature.is_some() && !signed {
            return;
        }
        marks.prompt = match kind {
            "A" | "D" => {
                // A new prompt without a command after the last one: the shell no longer
                // marks its commands, as when the profile took Return for itself.
                if marks.prompt == Prompt::Command {
                    marks.commands_marked = false;
                }
                if kind == "A" { Prompt::Prompt } else { Prompt::Finished }
            }
            "B" => Prompt::Command,
            "C" => {
                marks.commands_marked = signed;
                Prompt::Running
            }
            _ => marks.prompt,
        };
    }
}

/// A directory a shell may report: absolute and not on the network.
fn acceptable(path: PathBuf) -> Option<PathBuf> {
    let text = path.to_string_lossy();
    let remote = text.starts_with("\\\\") || text.starts_with("//");
    (path.is_absolute() && !remote).then_some(path)
}

/// The path of a `file://` address on this computer. A shell on another host, reached
/// through ssh, reports a directory there, which is no place to open a terminal in.
fn local_directory(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let local =
        host.is_empty() || host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case(&crate::process::host());
    if !local {
        return None;
    }
    let path = percent_decoded(path)?;
    // `file:///C:/Users` on Windows.
    let path = match path.get(1..3) {
        Some(drive) if cfg!(windows) && drive.ends_with(':') => path.get(1..).unwrap_or_default().to_string(),
        _ => path,
    };
    Some(PathBuf::from(path))
}

/// `%20` and the like as bytes, or `None` for a broken escape or text that is no UTF-8.
pub(crate) fn percent_decoded(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = after.get(..2)?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = after.get(2..).unwrap_or_default();
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    String::from_utf8(bytes).ok()
}

/// A PTY whose output passes a [`Scanner`] on its way to the emulator.
pub struct Tee<P> {
    inner: P,
    scanner: Scanner,
    marks: Arc<Mutex<Marks>>,
    at_shell: Box<dyn Fn() -> bool + Send>,
}

impl<P> Tee<P> {
    /// `at_shell` says whether the shell itself is in the foreground; `key` signs the marks
    /// of the shell, see [`Scanner::new`].
    pub fn new(inner: P, marks: Arc<Mutex<Marks>>, at_shell: Box<dyn Fn() -> bool + Send>, key: Option<&str>) -> Self {
        Self {
            inner,
            scanner: Scanner::new(key),
            marks,
            at_shell,
        }
    }
}

impl<P: EventedReadWrite> io::Read for Tee<P> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.reader().read(buffer)?;
        let bytes = buffer.get(..count).unwrap_or_default();
        self.scanner.scan(bytes, &self.marks, &*self.at_shell);
        Ok(count)
    }
}

impl<P: EventedReadWrite> EventedReadWrite for Tee<P> {
    type Reader = Self;
    type Writer = P::Writer;

    unsafe fn register(&mut self, poll: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()> {
        // SAFETY: the caller keeps the sources registered no longer than they live, which
        // are those of `inner`, owned by this.
        unsafe { self.inner.register(poll, interest, mode) }
    }

    fn reregister(&mut self, poll: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()> {
        self.inner.reregister(poll, interest, mode)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        self.inner.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self {
        self
    }

    fn writer(&mut self) -> &mut P::Writer {
        self.inner.writer()
    }
}

impl<P: EventedPty> EventedPty for Tee<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.inner.next_child_event()
    }
}

impl<P: OnResize> OnResize for Tee<P> {
    fn on_resize(&mut self, size: WindowSize) {
        self.inner.on_resize(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marks after `pieces`, with the shell in the foreground.
    fn scanned(pieces: &[&[u8]]) -> Marks {
        scanned_with(pieces, true)
    }

    fn scanned_with(pieces: &[&[u8]], at_shell: bool) -> Marks {
        let marks = Mutex::default();
        let mut scanner = Scanner::default();
        for piece in pieces {
            scanner.scan(piece, &marks, &|| at_shell);
        }
        marks.into_inner().unwrap()
    }

    /// The root of an absolute path in a `file:` address: Windows needs a drive.
    fn url_root() -> &'static str {
        if cfg!(windows) { "/C:" } else { "" }
    }

    /// An absolute path on this system.
    fn absolute(path: &str) -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_string()
        })
    }

    /// OSC 7 for `path`, which starts with a slash.
    fn osc_7(path: &str) -> String {
        format!("\x1b]7;file://{}{path}\x07", url_root())
    }

    /// The start of a prompt, after which a directory counts.
    const PROMPT: &[u8] = b"\x1b]133;D;0\x07\x1b]133;A\x07";

    #[test]
    fn osc_7_names_the_directory() {
        let osc = format!("$ \x1b]7;file://{}/home/ada/My%20Files\x07", url_root());
        let marks = scanned(&[PROMPT, osc.as_bytes()]);
        assert_eq!(marks.directory, Some(absolute("/home/ada/My Files")));
        let osc = format!("\x1b]7;file://localhost{}/srv\x1b\\", url_root());
        let local = scanned(&[PROMPT, osc.as_bytes()]);
        assert_eq!(local.directory, Some(absolute("/srv")));
    }

    #[test]
    fn a_directory_on_another_host_does_not_count() {
        assert_eq!(
            scanned(&[b"\x1b]7;file://elsewhere.example.org/srv\x07"]).directory,
            None
        );
        assert_eq!(scanned(&[b"\x1b]7;file:///broken%2\x07"]).directory, None);
        assert_eq!(scanned(&[b"\x1b]7;https://example.org/\x07"]).directory, None);
    }

    #[test]
    #[cfg(windows)]
    fn osc_9_9_names_the_directory_of_windows_terminal() {
        let marks = scanned(&[PROMPT, b"\x1b]9;9;\"C:\\Users\\ada\"\x1b\\"]);
        assert_eq!(marks.directory, Some(PathBuf::from("C:\\Users\\ada")));
        assert_eq!(scanned(&[b"\x1b]9;9;\x07"]).directory, None);
    }

    #[test]
    fn osc_9_9_names_an_absolute_directory() {
        let osc = format!("\x1b]9;9;\"{}\"\x07", absolute("/srv/a b").display());
        assert_eq!(scanned(&[PROMPT, osc.as_bytes()]).directory, Some(absolute("/srv/a b")));
        assert_eq!(scanned(&[b"\x1b]9;9;relative\x07"]).directory, None);
        assert_eq!(scanned(&[b"\x1b]9;9;\x07"]).directory, None);
    }

    #[test]
    fn shares_on_the_network_are_refused() {
        assert_eq!(scanned(&[b"\x1b]9;9;\\\\server\\share\x07"]).directory, None);
        assert_eq!(scanned(&[b"\x1b]9;9;//server/share\x07"]).directory, None);
        assert_eq!(scanned(&[b"\x1b]7;file:////server/share\x07"]).directory, None);
    }

    #[test]
    fn the_output_of_a_command_cannot_move_the_directory() {
        let marks = scanned(&[b"\x1b]133;C\x07", osc_7("/etc").as_bytes()]);
        assert_eq!(marks.directory, None);
        let osc = format!("\x1b]133;B\x07\x1b]9;9;{}\x07", absolute("/etc").display());
        let typing = scanned(&[osc.as_bytes()]);
        assert_eq!(typing.directory, None);
        let after = scanned(&[b"\x1b]133;C\x07", PROMPT, osc_7("/srv").as_bytes()]);
        assert_eq!(after.directory, Some(absolute("/srv")));
    }

    /// The marks after `pieces` for a shell that signs them with `KEY`.
    fn signed(pieces: &[&str]) -> Marks {
        let marks = Mutex::default();
        let mut scanner = Scanner::new(Some(KEY));
        for piece in pieces {
            scanner.scan(piece.as_bytes(), &marks, &|| true);
        }
        marks.into_inner().unwrap()
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// OSC 133 `kind`, signed with `KEY`.
    fn mark(kind: &str) -> String {
        format!("\x1b]133;{kind};kudari={KEY}\x1b\\")
    }

    #[test]
    fn only_a_shell_that_marks_its_commands_is_at_the_command_line() {
        // The Command Prompt: A and B, but never C.
        let cmd = signed(&[&mark("D"), &mark("A"), &mark("B")]);
        assert_eq!(cmd.prompt, Prompt::Command);
        assert!(!cmd.at_command_line());
        let pwsh = signed(&[&mark("C"), &mark("D;0"), &mark("A"), &mark("B")]);
        assert!(pwsh.at_command_line());
        let running = signed(&[&mark("B"), &mark("C")]);
        assert!(!running.at_command_line());
    }

    #[test]
    fn marks_without_the_key_of_the_terminal_do_not_count() {
        let pwsh = [mark("C"), mark("D;0"), mark("A"), mark("B")];
        // Output that a command shows while it runs, as from a file.
        let faked = [
            "\x1b]133;A\x07",
            "\x1b]133;B;kudari=fedcba9876543210fedcba9876543210\x07",
            "\x1b]133;B;kudari=\x07",
            "\x1b]133;Bkudari=0123456789abcdef0123456789abcdef\x07",
        ];
        for fake in faked {
            let marks = signed(&[&pwsh[0], fake]);
            assert_eq!(marks.prompt, Prompt::Running, "{fake:?}");
            assert!(!marks.at_command_line());
        }
        let shown = signed(&[&pwsh[0], &pwsh[1], &pwsh[2], &pwsh[3], &mark("C"), "\x1b]133;B\x07"]);
        assert_eq!(shown.prompt, Prompt::Running);
    }

    #[test]
    fn unsigned_marks_never_put_the_shell_at_the_command_line() {
        let unsigned = scanned(&[b"\x1b]133;C\x07\x1b]133;D;0\x07\x1b]133;A\x07\x1b]133;B\x07"]);
        assert_eq!(unsigned.prompt, Prompt::Command);
        assert!(!unsigned.at_command_line());
    }

    #[test]
    fn a_prompt_without_a_marked_command_withdraws_the_trust() {
        // The profile took Return for itself after the integration: the next line runs
        // without C, and the next prompt follows the command line.
        let marks = signed(&[
            &mark("C"),
            &mark("D;0"),
            &mark("A"),
            &mark("B"),
            &mark("D;0"),
            &mark("A"),
            &mark("B"),
        ]);
        assert_eq!(marks.prompt, Prompt::Command);
        assert!(!marks.at_command_line());
        let again = signed(&[
            &mark("C"),
            &mark("D;0"),
            &mark("A"),
            &mark("B"),
            &mark("A"),
            &mark("B"),
            &mark("C"),
        ]);
        assert!(again.commands_marked);
    }

    #[test]
    fn a_shell_without_marks_reports_no_directory() {
        assert_eq!(scanned(&[osc_7("/srv").as_bytes()]).directory, None);
        let osc = format!("\x1b]133;D;0\x07\x1b]9;9;{}\x07", absolute("/srv").display());
        assert_eq!(scanned(&[osc.as_bytes()]).directory, None);
    }

    #[test]
    fn a_program_in_the_foreground_cannot_fake_a_prompt() {
        let faked = scanned_with(&[PROMPT, osc_7("/srv").as_bytes()], false);
        assert_eq!(faked.directory, None);
        assert_eq!(faked.prompt, Prompt::Prompt);
    }

    #[test]
    fn osc_133_follows_the_prompt() {
        let marks = Mutex::default();
        let mut scanner = Scanner::default();
        let mut after = |bytes: &[u8]| {
            scanner.scan(bytes, &marks, &|| true);
            marks.lock().unwrap().prompt
        };
        assert_eq!(after(b"plain output"), Prompt::Unknown);
        assert_eq!(after(b"\x1b]133;A\x07$ "), Prompt::Prompt);
        assert_eq!(after(b"\x1b]133;B\x07"), Prompt::Command);
        assert_eq!(after(b"ls\r\n\x1b]133;C\x07"), Prompt::Running);
        assert_eq!(after(b"a b\r\n\x1b]133;D;0\x07"), Prompt::Finished);
        assert_eq!(after(b"\x1b]133;Z\x07"), Prompt::Finished);
    }

    #[test]
    fn sequences_can_be_split_anywhere() {
        let whole = format!("x\x1b]7;file://{}/tmp/a\x1b\\y", url_root());
        let whole = whole.as_bytes();
        for split in 0..whole.len() {
            let (first, second) = whole.split_at(split);
            assert_eq!(
                scanned(&[PROMPT, first, second]).directory,
                Some(absolute("/tmp/a")),
                "{split}"
            );
        }
    }

    #[test]
    fn other_sequences_and_overlong_ones_leave_the_marks_alone() {
        assert_eq!(
            scanned(&[b"\x1b[31mred\x1b]0;title\x07\x1b]8;;https://kde.org\x1b\\"]),
            Marks::default()
        );
        let mut long = b"\x1b]7;file:///".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_SEQUENCE));
        long.push(0x07);
        assert_eq!(scanned(&[&long]).directory, None);
        // A new escape inside a sequence that is not its terminator ends it.
        assert_eq!(scanned(&[b"\x1b]7;file:///a\x1b[0m\x07"]).directory, None);
        assert_eq!(scanned(&[b"\x1b\x1b]133;A\x07"]).prompt, Prompt::Prompt);
    }

    #[test]
    fn random_output_does_no_harm() {
        for input in crate::fuzz::inputs(&["\x1b]7;file:///tmp\x07", "\x1b]133;B\x1b\\", "\x1b]9;9;C:\\\x07"]) {
            let marks = Mutex::default();
            Scanner::default().scan(input.as_bytes(), &marks, &|| true);
            let _ = percent_decoded(&input);
            let _ = local_directory(&input);
        }
    }

    /// A PTY that has its output ready and keeps what is written.
    struct Fake {
        output: io::Cursor<Vec<u8>>,
        input: Vec<u8>,
        resized: bool,
    }

    impl EventedReadWrite for Fake {
        type Reader = io::Cursor<Vec<u8>>;
        type Writer = Vec<u8>;

        unsafe fn register(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
            Ok(())
        }

        fn reregister(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()> {
            Ok(())
        }

        fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()> {
            Ok(())
        }

        fn reader(&mut self) -> &mut Self::Reader {
            &mut self.output
        }

        fn writer(&mut self) -> &mut Self::Writer {
            &mut self.input
        }
    }

    impl EventedPty for Fake {
        fn next_child_event(&mut self) -> Option<ChildEvent> {
            Some(ChildEvent::Exited(None))
        }
    }

    impl OnResize for Fake {
        fn on_resize(&mut self, _: WindowSize) {
            self.resized = true;
        }
    }

    #[test]
    fn the_emulator_gets_every_byte_and_the_marks_are_read() {
        use std::io::{Read, Write};

        let output = b"a\x1b]133;B\x07b".to_vec();
        let marks = Arc::default();
        let fake = Fake {
            output: io::Cursor::new(output.clone()),
            input: Vec::new(),
            resized: false,
        };
        let mut tee = Tee::new(fake, Arc::clone(&marks), Box::new(|| true), None);
        let mut read = Vec::new();
        tee.reader().read_to_end(&mut read).unwrap();
        assert_eq!(read, output);
        assert_eq!(marks.lock().unwrap().prompt, Prompt::Command);
        tee.writer().write_all(b"ls").unwrap();
        assert_eq!(tee.inner.input, b"ls");
        assert_eq!(tee.next_child_event(), Some(ChildEvent::Exited(None)));
        tee.on_resize(WindowSize {
            num_lines: 1,
            num_cols: 1,
            cell_width: 1,
            cell_height: 1,
        });
        assert!(tee.inner.resized);
        let poller = Arc::new(Poller::new().unwrap());
        // SAFETY: the fake registers nothing.
        unsafe { tee.register(&poller, Event::none(0), PollMode::Level).unwrap() };
        tee.reregister(&poller, Event::none(0), PollMode::Level).unwrap();
        tee.deregister(&poller).unwrap();
    }
}
