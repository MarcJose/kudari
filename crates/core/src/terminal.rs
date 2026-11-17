// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A running shell with its emulator state. The front end hears from the terminals through
//! a [`Listener`], which it installs with [`set_listener`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{ClipboardType, Config, MIN_COLUMNS, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{CursorStyle, Handler, Processor, Rgb, StdSyncHandler};

use crate::colors::ColorScheme;
use crate::command::Quoting;
use crate::marks::{Marks, Prompt, Tee};
#[cfg(windows)]
use crate::process::Console;
#[cfg(unix)]
use crate::process::Pty;
use crate::process::{self, Programs, Signal};
use crate::services::{Clipboard, Desktop, Selection};
use crate::sessions::{SessionId, TerminalId};
use crate::title;

pub use crate::options::Options;

/// What a terminal tells its front end.
#[derive(Debug)]
pub enum Report {
    /// New output to show. More output is reported only once this is dropped, so the front
    /// end drops it right before it reads the terminal: output that arrives meanwhile is
    /// reported anew.
    Output(Pending),
    /// The bell rang. Further bells are reported only once this is dropped.
    Bell(Pending),
    /// The shell ended.
    Exited,
}

/// Holds back further reports of the same kind while it lives.
#[derive(Debug)]
pub struct Pending(Arc<AtomicBool>);

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// How the terminals reach their front end. They call it from the threads of their PTYs.
pub trait Listener: Send + Sync {
    /// Runs `work` later on the thread of the user interface, which owns the terminals.
    fn run(&self, work: Box<dyn FnOnce() + Send>);
    /// Hands a report to the thread of the user interface.
    fn report(&self, terminal: TerminalId, report: Report);
    /// The clipboard, on the thread of the user interface, for programs that copy or paste.
    fn clipboard(&self) -> &dyn Clipboard;
}

static LISTENER: OnceLock<&'static dyn Listener> = OnceLock::new();

/// Installs the front end that hears from the terminals. Only the first call counts.
pub fn set_listener(listener: &'static dyn Listener) {
    let _ = LISTENER.set(listener);
}

fn listener() -> Option<&'static dyn Listener> {
    LISTENER.get().copied()
}

/// Runs `work` on the thread of the user interface, if a front end listens.
fn run(work: impl FnOnce() + Send + 'static) {
    if let Some(listener) = listener() {
        listener.run(Box::new(work));
    }
}

static OPTIONS: Mutex<Option<Options>> = Mutex::new(None);
/// Width of the scrollbar column; zero while the scrollbar is hidden.
static SCROLLBAR_SPACE: AtomicU32 = AtomicU32::new(SCROLLBAR_WIDTH as u32);

pub fn options() -> Option<Options> {
    OPTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn config() -> Config {
    let mut config = Config::default();
    if let Some(options) = options() {
        config.scrolling_history = options.history_lines;
        config.default_cursor_style = CursorStyle {
            shape: options.cursor,
            blinking: options.blinking,
        };
        config.kitty_keyboard = options.kitty_keyboard;
        if !options.word_separators.is_empty() {
            config.semantic_escape_chars = options.word_separators;
        }
    }
    config
}

/// The space the scrollbar takes next to the text.
pub fn set_scrollbar_visible(visible: bool) {
    SCROLLBAR_SPACE.store(if visible { SCROLLBAR_WIDTH as u32 } else { 0 }, Ordering::Relaxed);
}

pub fn scrollbar_space() -> f64 {
    f64::from(SCROLLBAR_SPACE.load(Ordering::Relaxed))
}

/// Variables for the shell. `alacritty_terminal::tty::setup_env` would pick
/// `TERM=alacritty`, which remote hosts usually do not know, so this uses Konsole's values.
fn shell_environment(custom: &str) -> HashMap<String, String> {
    let mut environment = HashMap::from([
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        // `alacritty_terminal` sets both to the number of the terminal, which is no window of
        // X11: `w3mimgdisplay` or `xdotool` would draw into or act on another window, and tools
        // would take Kudari for Alacritty.
        ("WINDOWID".to_string(), String::new()),
        ("ALACRITTY_WINDOW_ID".to_string(), String::new()),
    ]);
    environment.extend(parse_environment(custom));
    environment
}

/// Reads `NAME=value` lines; anything else is ignored.
fn parse_environment(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    text.lines().filter_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        valid.then(|| (name.to_string(), value.to_string()))
    })
}

pub fn apply_options(options: Options) {
    let flow_control = options.flow_control;
    let before = OPTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(options);
    // Programs such as `nano` and `emacs` switch flow control off for themselves; setting it
    // again on every change of the settings or the theme would take Ctrl+S from them.
    let changed = before.is_none_or(|before| before.flow_control != flow_control);
    for backend in backends().values() {
        backend.apply_config();
        if changed {
            backend.apply_flow_control();
        }
    }
}

/// Space around the text, as measured in Konsole.
pub const TEXT_MARGIN_X: f64 = 4.0;
pub const TEXT_MARGIN_Y: f64 = 1.0;
/// Width of the scrollbar next to the text (Breeze's `PM_ScrollBarExtent`).
pub const SCROLLBAR_WIDTH: f64 = 21.0;

/// Space around the text, including the configured margin.
fn text_margin() -> (f64, f64) {
    let extra = options().map_or(0.0, |options| f64::from(options.margin));
    (TEXT_MARGIN_X + extra, TEXT_MARGIN_Y + extra)
}

/// Top left corner of the grid inside an item of the given size.
#[must_use]
pub fn text_origin(width: f64, height: f64, columns: f64, lines: f64) -> (f64, f64) {
    let (x, y) = text_margin();
    if options().is_some_and(|options| options.center) {
        centered(width, height, columns, lines, (x, y))
    } else {
        (x, y)
    }
}

/// The corner of a grid centered in an item, at least `margin` from its edges, on whole
/// pixels: the backgrounds of the cells and their glyphs round to the same ones then.
fn centered(width: f64, height: f64, columns: f64, lines: f64, margin: (f64, f64)) -> (f64, f64) {
    (
        ((width - columns).max(2.0 * margin.0) / 2.0).floor(),
        ((height - lines).max(2.0 * margin.1) / 2.0).floor(),
    )
}

/// Columns and lines of a terminal pane of the given pixel size. The shell has to
/// start with this size, or it would redraw its prompt on the first resize.
#[must_use]
pub fn grid_size(width: i32, height: i32, cell_width: f64, cell_height: f64) -> (u16, u16) {
    let (margin_x, margin_y) = text_margin();
    let columns = (f64::from(width) - scrollbar_space() - 2.0 * margin_x) / cell_width;
    let lines = (f64::from(height) - 2.0 * margin_y) / cell_height;
    (
        least_columns((columns as i32).clamp(1, 4096) as u16),
        (lines as i32).clamp(1, 4096) as u16,
    )
}

/// At least the columns that `alacritty_terminal` needs: with fewer, it wraps a line with a
/// wide character again and again when it shrinks, and the window freezes while the memory runs out.
/// A pane that narrow cuts off the second column, which does no harm.
fn least_columns(columns: u16) -> u16 {
    columns.max(MIN_COLUMNS as u16)
}

/// An answer that a program asked for, in the order it asked. Some need the thread of the
/// user interface, so all of them go there, or an answer would overtake an earlier one:
/// programs ask for a color and then for the device attributes, and take the answer to
/// the second as a sign that the first has none.
/// The most answers that wait for the user interface. A program that asks faster than the user
/// interface answers, as `yes $'\e[c'` while the window is busy, would otherwise make the list
/// grow without end; more questions than this get no answer, as from a terminal that is slow.
const MAX_REPLIES: usize = 4096;

enum Reply {
    Text(String),
    /// One of the colors, by its number in [`Colors`].
    Color(usize, Arc<dyn Fn(Rgb) -> String + Send + Sync>),
    /// The size of the text area in pixels.
    Size(Arc<dyn Fn(WindowSize) -> String + Send + Sync>),
    /// The text of the clipboard.
    Paste(Arc<dyn Fn(&str) -> String + Send + Sync>),
}

/// What the thread of a PTY leaves for the thread of the user interface. A program can ring
/// the bell, set its title or copy text a million times a second; the user interface hears
/// of it once until it has taken what is there, instead of once each time.
#[derive(Default)]
struct Mailbox {
    repaint_queued: Arc<AtomicBool>,
    bell_queued: Arc<AtomicBool>,
    /// Title set by the running program.
    title: Mutex<String>,
    /// The text the program copied last, until the user interface takes it.
    copied: Mutex<Option<String>>,
    /// The same for the primary selection, which a middle click pastes.
    selected: Mutex<Option<String>>,
    replies: Mutex<Vec<Reply>>,
}

/// The most bytes of a title that a program sets: tabs, the window and notifications show
/// little of it, and a longer one only takes time to copy and lay out.
const MAX_TITLE: usize = 1024;

/// A title that a program set, cut to [`MAX_TITLE`] at the end of a character, with spaces
/// for control characters.
fn bounded_title(title: &str) -> String {
    let mut end = title.len().min(MAX_TITLE);
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    title[..end]
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect()
}

impl Mailbox {
    /// Where the text copied for `target` waits.
    fn slot(&self, target: Selection) -> &Mutex<Option<String>> {
        match target {
            Selection::Clipboard => &self.copied,
            Selection::Primary => &self.selected,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
pub struct Proxy {
    id: TerminalId,
    mailbox: Arc<Mailbox>,
}

impl Proxy {
    /// Queues an answer, and the work that sends the answers if none is queued yet.
    fn reply(&self, reply: Reply) {
        let first = {
            let mut replies = lock(&self.mailbox.replies);
            if replies.len() >= MAX_REPLIES {
                return;
            }
            replies.push(reply);
            replies.len() == 1
        };
        if first {
            let id = self.id;
            run(move || {
                if let Some(backend) = backend(id) {
                    backend.answer();
                }
            });
        }
    }
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let id = self.id;
        let Some(listener) = listener() else {
            return;
        };
        let mailbox = &self.mailbox;
        match event {
            Event::Wakeup if !mailbox.repaint_queued.swap(true, Ordering::AcqRel) => {
                listener.report(id, Report::Output(Pending(mailbox.repaint_queued.clone())));
            }
            Event::Bell if !mailbox.bell_queued.swap(true, Ordering::AcqRel) => {
                listener.report(id, Report::Bell(Pending(mailbox.bell_queued.clone())));
            }
            // The titles are read with the output, so they need no work of their own.
            Event::Title(title) => *lock(&mailbox.title) = bounded_title(&title),
            Event::ResetTitle => lock(&mailbox.title).clear(),
            // OSC 52 with `p` or `s`, as `tmux` and `neovim` send it, names the primary selection.
            Event::ClipboardStore(kind, text) => {
                let target = match kind {
                    ClipboardType::Clipboard => Selection::Clipboard,
                    ClipboardType::Selection => Selection::Primary,
                };
                if lock(mailbox.slot(target)).replace(text).is_none() {
                    let mailbox = Arc::clone(mailbox);
                    run(move || {
                        if let Some(text) = lock(mailbox.slot(target)).take() {
                            listener.clipboard().copy(&text, target);
                        }
                    });
                }
            }
            // The flags of the kitty keyboard protocol in force, which `alacritty_terminal`
            // reports with those that Kudari does not carry out.
            Event::PtyWrite(text) => self.reply(Reply::Text(
                crate::input::supported_keyboard_report(&text).unwrap_or(text),
            )),
            Event::ColorRequest(index, format) => self.reply(Reply::Color(index, format)),
            Event::TextAreaSizeRequest(format) => self.reply(Reply::Size(format)),
            Event::ClipboardLoad(_, format) => self.reply(Reply::Paste(format)),
            Event::ChildExit(_) | Event::Exit => listener.report(id, Report::Exited),
            _ => {}
        }
    }
}

/// The color a program asks for by its number in [`Colors`]: the one it set itself, else
/// the one of the scheme, as the terminal draws it. `None` for a number without a color.
fn requested_color(index: usize, overrides: &Colors, scheme: &ColorScheme, cursor: Option<Rgb>) -> Option<Rgb> {
    if index >= COUNT {
        return None;
    }
    overrides[index].or_else(|| match index {
        0..=255 => Some(crate::colors::indexed_color(index as u8, scheme)),
        256 => Some(scheme.foreground),
        257 => Some(scheme.background),
        258 => Some(cursor.unwrap_or(scheme.foreground)),
        _ => None,
    })
}

/// What a terminal knows to answer with, for [`answers`].
struct Answering<'a> {
    overrides: &'a Colors,
    scheme: &'a ColorScheme,
    cursor: Option<Rgb>,
    size: WindowSize,
    paste: &'a dyn Fn() -> String,
}

/// The bytes that answer `replies`, in their order.
fn answers(replies: Vec<Reply>, answering: &Answering) -> Vec<u8> {
    let mut bytes = Vec::new();
    for reply in replies {
        let text = match reply {
            Reply::Text(text) => text,
            Reply::Color(index, format) => {
                match requested_color(index, answering.overrides, answering.scheme, answering.cursor) {
                    Some(color) => format(color),
                    None => continue,
                }
            }
            Reply::Size(format) => format(answering.size),
            Reply::Paste(format) => format(&(answering.paste)()),
        };
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

/// What the operating system tells about the programs of a terminal.
#[cfg(unix)]
type System = Pty;
#[cfg(windows)]
type System = Console;

pub struct Backend {
    /// Taken through [`Backend::lock`], which puts a view out of place back first.
    term: Arc<FairMutex<Term<Proxy>>>,
    notifier: Notifier,
    /// The shell and what it runs.
    programs: Arc<System>,
    /// What the shell said about itself in its output.
    marks: Arc<Mutex<Marks>>,
    pub last_output: Mutex<Instant>,
    /// What the thread of the PTY left for the user interface.
    mailbox: Arc<Mailbox>,
    /// The size of the screen in cells and of a cell in pixels.
    window_size: Mutex<WindowSize>,
    /// File the terminal content is saved to regularly.
    pub auto_save: Mutex<Option<PathBuf>>,
    /// Scrollback size of this terminal, instead of the configured one.
    history_lines: Mutex<Option<usize>>,
    /// Whether programs get the mouse here, instead of the configured choice.
    mouse_tracking: Mutex<Option<bool>>,
    /// How the shell of the terminal reads quoted paths.
    quoting: Quoting,
}

impl Backend {
    /// The terminal, locked, with its view where it can be: `alacritty_terminal` can leave the
    /// view scrolled back past the history, or the cursor of the selection mode outside the
    /// grid, and reading there would end Kudari (see [`view::repair_view`]).
    pub fn lock(&self) -> impl std::ops::DerefMut<Target = Term<Proxy>> + '_ {
        let mut term = self.term.lock();
        crate::view::repair_view(&mut term);
        term
    }

    fn spawn(
        id: TerminalId,
        columns: u16,
        lines: u16,
        cell_width: u16,
        cell_height: u16,
        working_directory: Option<PathBuf>,
        shell: Option<&str>,
    ) -> Result<Arc<Self>, String> {
        let columns = least_columns(columns);
        let mailbox = Arc::<Mailbox>::default();
        let proxy = Proxy {
            id,
            mailbox: Arc::clone(&mailbox),
        };
        let term = Term::new(config(), &TermSize::new(columns.into(), lines.into()), proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let size = WindowSize {
            num_lines: lines,
            num_cols: columns,
            cell_width,
            cell_height,
        };
        let settings = options().unwrap_or_default();
        let prompt = std::env::var("PROMPT").ok();
        let launch = crate::shells::launch(
            &settings.command,
            shell,
            &settings.shell,
            settings.shell_integration,
            crate::shells::installed(),
            prompt.as_deref(),
        )
        .unwrap_or_default();
        let mut environment = shell_environment(&settings.environment);
        let launch_key = launch.key;
        let mut quoting = if launch.program.is_empty() {
            Quoting::system()
        } else {
            launch.quoting
        };
        environment.extend(launch.environment);
        let configured = Some(PathBuf::from(crate::command::expand_home(&settings.directory)))
            .filter(|directory| directory.is_dir());
        let mut options = tty::Options {
            shell: (!launch.program.is_empty()).then(|| tty::Shell::new(launch.program, launch.arguments)),
            working_directory: configured.or(working_directory),
            env: environment,
            ..tty::Options::default()
        };
        let pty = match tty::new(&options, size, id as u64) {
            Ok(pty) => pty,
            // A broken command must not leave the user without a terminal.
            Err(error) if options.shell.is_some() => {
                let warning = crate::i18n::tr_arg(
                    "@info:shell",
                    "Could not start the configured command: %1",
                    &error.to_string(),
                );
                let mut parser = Processor::<StdSyncHandler>::new();
                parser.advance(&mut *term.lock(), format!("{warning}\r\n").as_bytes());
                options.shell = None;
                quoting = Quoting::system();
                tty::new(&options, size, id as u64).map_err(|error| error.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        #[cfg(unix)]
        let programs = Pty::of(pty.file(), pty.child().id() as i32).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        let programs = Console::new(pty.child_watcher().pid().map_or(0, std::num::NonZeroU32::get));

        let programs = Arc::new(programs);
        let marks = Arc::default();
        let shell = Arc::clone(&programs);
        let pty = Tee::new(
            pty,
            Arc::clone(&marks),
            Box::new(move || !shell.is_busy()),
            launch_key.as_deref(),
        );
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false).map_err(|error| error.to_string())?;
        let notifier = Notifier(event_loop.channel());
        // `alacritty_terminal` ends the process when it cannot start the thread of the PTY, as
        // when the user has as many processes as allowed: Kudari would end with all its
        // terminals. A thread that just started tells that one can.
        std::thread::Builder::new()
            .name("probe".to_string())
            .spawn(|| {})
            .map_err(|error| format!("no thread for the terminal: {error}"))?
            .join()
            .map_err(|_| "no thread for the terminal".to_string())?;
        event_loop.spawn();

        let backend = Arc::new(Self {
            term,
            notifier,
            programs,
            marks,
            last_output: Mutex::new(Instant::now()),
            mailbox,
            window_size: Mutex::new(size),
            auto_save: Mutex::new(None),
            history_lines: Mutex::new(None),
            mouse_tracking: Mutex::new(None),
            quoting,
        });
        backend.apply_flow_control();
        Ok(backend)
    }

    /// A path quoted for the shell of this terminal, or `None` if it cannot take it safely;
    /// see [`crate::command::quote_path`].
    #[must_use]
    pub fn quote_path(&self, path: &str) -> Option<String> {
        crate::command::quote_path(path, self.quoting)
    }

    fn apply_config(&self) {
        let mut config = config();
        if let Some(lines) = *self
            .history_lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            config.scrolling_history = lines;
        }
        self.lock().set_options(config);
    }

    /// The scrollback size of this terminal.
    pub fn history_lines(&self) -> usize {
        let own = *self
            .history_lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        own.unwrap_or_else(|| config().scrolling_history)
    }

    /// Sets the scrollback size of this terminal alone. Shrinking drops the oldest lines.
    pub fn set_history_lines(&self, lines: usize) {
        *self
            .history_lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(lines);
        self.apply_config();
    }

    pub fn tracks_mouse(&self) -> bool {
        let own = *self
            .mouse_tracking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        own.unwrap_or_else(|| options().is_none_or(|options| options.mouse_tracking))
    }

    pub fn set_tracks_mouse(&self, enabled: bool) {
        *self
            .mouse_tracking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(enabled);
    }

    /// Ctrl+S and Ctrl+Q stop and resume output only with flow control.
    fn apply_flow_control(&self) {
        self.programs
            .set_flow_control(options().is_none_or(|options| options.flow_control));
    }

    pub fn write(&self, bytes: Vec<u8>) {
        self.notifier.notify(bytes);
    }

    /// Sends the program the answers it asked for, on the thread of the user interface.
    fn answer(&self) {
        let replies = std::mem::take(&mut *lock(&self.mailbox.replies));
        let overrides = if replies.iter().any(|reply| matches!(reply, Reply::Color(..))) {
            *self.lock().colors()
        } else {
            Colors::default()
        };
        let paste = || listener().map_or_else(String::new, |listener| listener.clipboard().paste(Selection::Clipboard));
        let answering = Answering {
            overrides: &overrides,
            scheme: &crate::colors::active(),
            cursor: options().and_then(|options| options.cursor_color),
            size: *lock(&self.window_size),
            paste: &paste,
        };
        let bytes = answers(replies, &answering);
        if !bytes.is_empty() {
            self.write(bytes);
        }
    }

    /// Input always returns the view to the bottom of the scrollback.
    pub fn write_user_input(&self, bytes: Vec<u8>) {
        self.lock().scroll_display(Scroll::Bottom);
        self.write(bytes);
    }

    pub fn resize(&self, columns: u16, lines: u16, cell_width: u16, cell_height: u16) {
        let columns = least_columns(columns);
        let size = WindowSize {
            num_lines: lines,
            num_cols: columns,
            cell_width,
            cell_height,
        };
        // A zoom changes the cells without changing their number; programs that ask for
        // the size in pixels get the new one.
        *lock(&self.window_size) = size;
        let mut term = self.lock();
        if term.columns() == usize::from(columns) && term.screen_lines() == usize::from(lines) {
            return;
        }
        term.resize(TermSize::new(columns.into(), lines.into()));
        drop(term);
        let mut notifier = Notifier(self.notifier.0.clone());
        notifier.on_resize(size);
    }

    /// The directory of the program in the foreground, or else the one the shell reported,
    /// if it is a directory still.
    pub fn working_directory(&self) -> Option<PathBuf> {
        self.programs.working_directory().or_else(|| {
            let marks = self.marks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            marks.directory.clone().filter(|directory| directory.is_dir())
        })
    }

    /// Whether the user types a command at the prompt, by the marks of the shell.
    pub fn at_command_line(&self) -> bool {
        self.marks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .at_command_line()
    }

    /// Where the shell is in its prompt, if it marks it.
    pub fn prompt(&self) -> Prompt {
        self.marks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .prompt
    }

    pub fn process_name(&self) -> Option<String> {
        self.programs.name()
    }

    fn program_title(&self) -> String {
        lock(&self.mailbox.title).clone()
    }

    /// The title of the terminal in `session`: the remote format while `ssh` runs, else
    /// `format`.
    pub fn title(&self, format: &str, session: SessionId) -> String {
        let remote = options().map_or_default(|options| options.remote_title_format);
        title::title(
            &*self.programs,
            self.working_directory(),
            &title::with_session(format, session),
            &title::with_session(&remote, session),
            &self.program_title(),
        )
    }

    /// The text of a format string; see [`title::format_title`].
    pub fn local_title(&self, format: &str) -> String {
        title::local_title(&*self.programs, self.working_directory(), format, &self.program_title())
    }

    /// True while a program runs in the foreground instead of the shell.
    pub fn is_busy(&self) -> bool {
        self.programs.is_busy()
    }

    /// The same for what is only shown; see [`Programs::is_busy_recent`].
    pub fn is_busy_recent(&self) -> bool {
        self.programs.is_busy_recent()
    }

    /// Sends a signal to the program in the foreground. A console has no signal for
    /// interrupting, so there the terminal types Ctrl+C.
    pub fn send_signal(&self, signal: Signal) {
        if cfg!(windows) && signal == Signal::Interrupt {
            self.write(vec![0x03]);
        } else {
            self.programs.signal(signal);
        }
    }

    /// Whether [`Backend::send_signal`] can send `signal` here.
    pub fn supports(&self, signal: Signal) -> bool {
        self.programs.supports(signal)
    }

    /// The whole scrollback as plain text.
    pub fn full_text(&self) -> String {
        let term = self.lock();
        let start = Point::new(term.grid().topmost_line(), Column(0));
        let end = Point::new(term.grid().bottommost_line(), term.grid().last_column());
        term.bounds_to_string(start, end)
    }

    /// The last `lines` lines of scrollback and screen, or the screen when `lines` is zero.
    pub fn recent_text(&self, lines: usize) -> String {
        last_lines(&self.lock(), lines)
    }

    /// Columns and lines of the screen.
    pub fn size(&self) -> (usize, usize) {
        let term = self.lock();
        (term.columns(), term.screen_lines())
    }

    /// Whether the program asked for the cursor keys of the application mode.
    pub fn app_cursor(&self) -> bool {
        self.lock().mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether the program asked for bracketed paste, as line editors such as those of
    /// bash, zsh and fish do while they edit a command line, and only then.
    pub fn bracketed_paste(&self) -> bool {
        self.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Forgets the scrollback and returns to the initial terminal state.
    pub fn reset(&self) {
        let mut term = self.lock();
        let full_screen = term.mode().contains(TermMode::ALT_SCREEN);
        term.reset_state();
        drop(term);
        if redraws_after_reset(self.is_busy(), full_screen) {
            self.write(vec![0x0c]);
        }
    }

    /// Terminal output can hold passwords and tokens, so a new file is private. A file
    /// that already exists keeps its permissions.
    ///
    /// # Errors
    /// What the file system reports.
    pub fn save_output(&self, path: &std::path::Path) -> std::io::Result<()> {
        process::write_private(path, self.full_text().as_bytes())
    }
}

/// Whether a reset types Ctrl+L, which makes the shell or a full-screen program draw itself
/// again. Other programs read it as input: `cat > notes` would write it into the file, and a
/// question for a password would take it for part of the password.
fn redraws_after_reset(busy: bool, full_screen: bool) -> bool {
    !busy || full_screen
}

/// What AI agents see of a terminal.
impl crate::agent::Terminal for Backend {
    fn size(&self) -> (usize, usize) {
        Backend::size(self)
    }

    fn title(&self, format: &str, session: SessionId) -> String {
        Backend::title(self, format, session)
    }

    fn working_directory(&self) -> Option<PathBuf> {
        Backend::working_directory(self)
    }

    fn process_name(&self) -> Option<String> {
        Backend::process_name(self)
    }

    fn is_busy(&self) -> bool {
        Backend::is_busy(self)
    }

    fn bracketed_paste(&self) -> bool {
        Backend::bracketed_paste(self)
    }

    fn at_command_line(&self) -> bool {
        Backend::at_command_line(self)
    }

    fn app_cursor(&self) -> bool {
        Backend::app_cursor(self)
    }

    fn recent_text(&self, lines: usize) -> String {
        Backend::recent_text(self, lines)
    }

    fn write_user_input(&self, bytes: Vec<u8>) {
        Backend::write_user_input(self, bytes);
    }
}

/// The text of the last `lines` lines that have any, up to the end of the screen; all of the
/// screen when `lines` is zero. The empty lines below the text do not count.
fn last_lines<T: EventListener>(term: &Term<T>, lines: usize) -> String {
    let grid = term.grid();
    let screen = term.screen_lines();
    // The screen comes along, since the text can end anywhere on it.
    let span = if lines == 0 {
        screen
    } else {
        lines.saturating_add(screen)
    };
    let bottom = grid.bottommost_line();
    // Line numbers run from minus the scrollback up to the last line of the screen.
    let count = i32::try_from(span).unwrap_or(i32::MAX).saturating_sub(1);
    let top = Line(bottom.0.saturating_sub(count)).max(grid.topmost_line());
    let text = term.bounds_to_string(Point::new(top, Column(0)), Point::new(bottom, grid.last_column()));
    let mut found: Vec<&str> = text.lines().map(str::trim_end).collect();
    let used = found
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |last| last + 1);
    found.truncate(used);
    let wanted = if lines == 0 { screen } else { lines };
    found.split_off(found.len().saturating_sub(wanted)).join("\n")
}

/// Types paths into a terminal, quoted for its shell, or a command that changes into a
/// directory. Paths the shell cannot take safely are left out.
pub fn send_paths(id: TerminalId, paths: &str, change_directory: bool) {
    if let Some(backend) = backend(id) {
        let quoted = paths
            .lines()
            .filter_map(|path| backend.quote_path(path))
            .collect::<Vec<_>>()
            .join(" ");
        if quoted.is_empty() {
            return;
        }
        let text = if change_directory {
            format!("{}\r", crate::command::change_directory(&quoted, backend.quoting))
        } else {
            format!("{quoted} ")
        };
        backend.write_user_input(text.into_bytes());
    }
}

/// Opens a link, with `:line:column` after a file going to the configured editor.
pub fn open_link(link: &str, desktop: &dyn Desktop) {
    let options = options().unwrap_or_default();
    crate::links::open(link, &options.editor_command, &options.link_schemes, desktop);
}

/// Shared with Qt's render thread, which calls `TerminalItem::paint`.
static BACKENDS: LazyLock<Mutex<HashMap<TerminalId, Arc<Backend>>>> = LazyLock::new(Mutex::default);

fn backends() -> std::sync::MutexGuard<'static, HashMap<TerminalId, Arc<Backend>>> {
    BACKENDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Looks up the backend of a terminal.
#[must_use]
pub fn backend(id: TerminalId) -> Option<Arc<Backend>> {
    backends().get(&id).cloned()
}

/// Starts the shell of a terminal.
///
/// # Errors
/// Why neither the configured command nor the default shell could start.
pub fn start(
    id: TerminalId,
    columns: u16,
    lines: u16,
    cell_width: u16,
    cell_height: u16,
    working_directory: Option<PathBuf>,
    shell: Option<&str>,
) -> Result<(), String> {
    let backend = Backend::spawn(id, columns, lines, cell_width, cell_height, working_directory, shell)?;
    backends().insert(id, backend);
    Ok(())
}

/// Writes the terminal content to its auto-save file, if it has one.
pub fn auto_save(id: TerminalId) {
    let Some(backend) = backend(id) else {
        return;
    };
    let path = backend.auto_save.lock().ok().and_then(|path| path.clone());
    if let Some(path) = path
        && let Err(error) = backend.save_output(&path)
    {
        eprintln!("kudari: could not save {}: {error}", path.display());
    }
}

/// Drops the backend, which ends the PTY loop and hangs up the shell.
pub fn stop(id: TerminalId) {
    let removed = backends().remove(&id);
    if let Some(backend) = removed {
        backend.programs.hang_up();
        let _ = backend.notifier.0.send(alacritty_terminal::event_loop::Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_text_does_no_harm() {
        for input in crate::fuzz::inputs(&["EDITOR=vim\nA=b"]) {
            let _ = parse_environment(&input).count();
        }
    }

    fn term_with(
        text: &str,
        columns: usize,
        lines: usize,
        history: usize,
    ) -> Term<alacritty_terminal::event::VoidListener> {
        let config = Config {
            scrolling_history: history,
            ..Config::default()
        };
        let mut term = Term::new(
            config,
            &TermSize::new(columns, lines),
            alacritty_terminal::event::VoidListener,
        );
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, text.replace('\n', "\r\n").as_bytes());
        term
    }

    std::thread_local! {
        static WORK: std::cell::RefCell<Vec<Box<dyn FnOnce() + Send>>> = std::cell::RefCell::default();
        static REPORTS: std::cell::RefCell<Vec<Report>> = std::cell::RefCell::default();
        static COPIED: std::cell::RefCell<Vec<String>> = std::cell::RefCell::default();
        static SELECTED: std::cell::RefCell<Vec<String>> = std::cell::RefCell::default();
    }

    /// A front end that keeps what it hears, for the test that runs on its thread.
    struct Kept;

    impl Listener for Kept {
        fn run(&self, work: Box<dyn FnOnce() + Send>) {
            WORK.with_borrow_mut(|kept| kept.push(work));
        }

        fn report(&self, _terminal: TerminalId, report: Report) {
            REPORTS.with_borrow_mut(|kept| kept.push(report));
        }

        fn clipboard(&self) -> &dyn Clipboard {
            self
        }
    }

    impl Clipboard for Kept {
        fn copy(&self, text: &str, target: Selection) {
            let kept = if target == Selection::Clipboard {
                &COPIED
            } else {
                &SELECTED
            };
            kept.with_borrow_mut(|kept| kept.push(text.to_string()));
        }

        fn copy_rich(&self, _text: &str, _html: &str) {}

        fn paste(&self, _source: Selection) -> String {
            "pasted".to_string()
        }
    }

    /// A proxy that reports to [`Kept`].
    fn kept_proxy() -> Proxy {
        set_listener(&Kept);
        Proxy {
            id: 0,
            mailbox: Arc::default(),
        }
    }

    /// Runs the work that the proxy left for the thread of the user interface.
    fn run_work() -> usize {
        let work = WORK.take();
        let count = work.len();
        for work in work {
            work();
        }
        count
    }

    #[test]
    fn a_flood_of_bells_is_reported_once_until_the_front_end_takes_it() {
        let proxy = kept_proxy();
        // `yes $'\a'`, or a binary file shown by mistake.
        for _ in 0..1000 {
            proxy.send_event(Event::Bell);
        }
        let mut reports = REPORTS.take();
        assert_eq!(reports.len(), 1, "one report for a thousand bells");
        assert!(matches!(reports[0], Report::Bell(_)));
        // Once the front end has the report, the next bell rings again.
        reports.clear();
        proxy.send_event(Event::Bell);
        assert_eq!(REPORTS.take().len(), 1);
    }

    #[test]
    fn titles_wait_in_the_terminal_without_work_for_the_front_end() {
        let proxy = kept_proxy();
        for number in 0..1000 {
            proxy.send_event(Event::Title(format!("title {number}")));
        }
        assert_eq!(run_work(), 0);
        assert_eq!(*lock(&proxy.mailbox.title), "title 999");
        proxy.send_event(Event::ResetTitle);
        assert_eq!(*lock(&proxy.mailbox.title), "");
    }

    #[test]
    fn only_the_text_copied_last_reaches_the_clipboard() {
        let proxy = kept_proxy();
        // OSC 52, a thousand times while the front end is busy.
        for number in 0..1000 {
            proxy.send_event(Event::ClipboardStore(
                alacritty_terminal::term::ClipboardType::Clipboard,
                format!("text {number}"),
            ));
        }
        assert_eq!(run_work(), 1);
        assert_eq!(COPIED.take(), ["text 999"]);
        // Text copied afterwards comes through as well.
        proxy.send_event(Event::ClipboardStore(
            alacritty_terminal::term::ClipboardType::Clipboard,
            "later".to_string(),
        ));
        assert_eq!(run_work(), 1);
        assert_eq!(COPIED.take(), ["later"]);
    }

    #[test]
    fn a_reset_types_ctrl_l_only_for_the_shell_or_a_full_screen_program() {
        assert!(redraws_after_reset(false, false), "the shell at its prompt");
        assert!(redraws_after_reset(true, true), "vim or htop");
        assert!(!redraws_after_reset(true, false), "cat or a question for a password");
    }

    #[test]
    fn titles_of_programs_have_a_bound() {
        let proxy = kept_proxy();
        // `printf '\e]2;%s\a' "$(head -c 50000000 /dev/zero | tr '\0' x)"`, in short.
        proxy.send_event(Event::Title("x".repeat(5_000_000)));
        assert_eq!(lock(&proxy.mailbox.title).len(), MAX_TITLE);
        // Cut at the end of a character, with spaces for control characters.
        proxy.send_event(Event::Title(format!("{}ä\ttab", "x".repeat(MAX_TITLE - 1))));
        assert_eq!(lock(&proxy.mailbox.title).len(), MAX_TITLE - 1);
        proxy.send_event(Event::Title("a\tb\u{7}c".to_string()));
        assert_eq!(*lock(&proxy.mailbox.title), "a b c");
    }

    #[test]
    fn copies_for_the_primary_selection_leave_the_clipboard() {
        let proxy = kept_proxy();
        // `printf '\e]52;p;aGk=\a'`, as tmux sends it for the primary selection.
        proxy.send_event(Event::ClipboardStore(ClipboardType::Selection, "hi".to_string()));
        proxy.send_event(Event::ClipboardStore(ClipboardType::Clipboard, "ho".to_string()));
        assert_eq!(run_work(), 2);
        assert_eq!(SELECTED.take(), ["hi"]);
        assert_eq!(COPIED.take(), ["ho"]);
    }

    /// What the terminal answers to `output`, with the default scheme, cells of 8 by 16
    /// pixels and 24 lines of 80 columns.
    fn answer_to(output: &str) -> (usize, String) {
        let proxy = kept_proxy();
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), proxy.clone());
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        let work = run_work();
        let replies = std::mem::take(&mut *lock(&proxy.mailbox.replies));
        let answering = Answering {
            overrides: term.colors(),
            scheme: &ColorScheme::default(),
            cursor: None,
            size: WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 8,
                cell_height: 16,
            },
            paste: &|| "pasted".to_string(),
        };
        (
            work,
            String::from_utf8(answers(replies, &answering)).unwrap_or_default(),
        )
    }

    #[test]
    fn questions_beyond_what_waits_get_no_answer() {
        // Far more questions than the user interface takes before it answers them.
        let (work, answer) = answer_to(&"\x1b[c".repeat(MAX_REPLIES * 3));
        assert_eq!(work, 1);
        assert_eq!(answer, "\x1b[?6c".repeat(MAX_REPLIES));
    }

    #[test]
    fn a_flood_of_questions_is_one_piece_of_work_for_the_front_end() {
        // `yes $'\e[c'`: a thousand questions for the device attributes.
        let (work, answer) = answer_to(&"\x1b[c".repeat(1000));
        assert_eq!(work, 1);
        assert_eq!(answer, "\x1b[?6c".repeat(1000));
    }

    #[test]
    fn programs_get_the_colors_and_the_size_they_ask_for() {
        // As `neovim` asks for the background, and an image viewer for the size in pixels.
        let (_, answer) = answer_to("\x1b]11;?\x1b\\");
        assert_eq!(answer, "\x1b]11;rgb:2323/2626/2727\x1b\\");
        let (_, answer) = answer_to("\x1b]10;?\x07\x1b]12;?\x07\x1b]4;1;?\x07");
        assert_eq!(
            answer,
            "\x1b]10;rgb:fcfc/fcfc/fcfc\x07\x1b]12;rgb:fcfc/fcfc/fcfc\x07\x1b]4;1;rgb:fcfc/fcfc/fcfc\x07"
        );
        let (_, answer) = answer_to("\x1b[14t");
        assert_eq!(answer, "\x1b[4;384;640t");
        // A color that the program set is the one it gets back.
        let (_, answer) = answer_to("\x1b]11;#102030\x1b\\\x1b]11;?\x1b\\");
        assert_eq!(answer, "\x1b]11;rgb:1010/2020/3030\x1b\\");
    }

    #[test]
    fn answers_keep_the_order_of_the_questions_with_one_piece_of_work() {
        // A color, then the device attributes, which programs take as the end of the answers.
        let (work, answer) = answer_to("\x1b]11;?\x1b\\\x1b[c\x1b[14t\x1b[c");
        assert_eq!(work, 1);
        assert_eq!(
            answer,
            "\x1b]11;rgb:2323/2626/2727\x1b\\\x1b[?6c\x1b[4;384;640t\x1b[?6c"
        );
    }

    #[test]
    fn colors_beyond_the_table_get_no_answer() {
        let scheme = ColorScheme::default();
        assert_eq!(requested_color(COUNT, &Colors::default(), &scheme, None), None);
        assert_eq!(requested_color(usize::MAX, &Colors::default(), &scheme, None), None);
        let cursor = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(
            requested_color(258, &Colors::default(), &scheme, Some(cursor)),
            Some(cursor)
        );
    }

    #[test]
    fn recent_text_ends_with_the_screen() {
        let term = term_with("one\ntwo\nthree\nfour\nfive\n$ ", 20, 3, 100);
        assert_eq!(last_lines(&term, 0), "four\nfive\n$");
        assert_eq!(last_lines(&term, 2), "five\n$");
        assert_eq!(last_lines(&term, 5), "two\nthree\nfour\nfive\n$");
        assert_eq!(last_lines(&term, 100), "one\ntwo\nthree\nfour\nfive\n$");
        // The lines below the prompt are left out, and do not count.
        assert_eq!(last_lines(&term_with("$ ls", 20, 5, 0), 0), "$ ls");
        assert_eq!(last_lines(&term_with("a\nb\n$ ls", 20, 5, 0), 2), "b\n$ ls");
        assert_eq!(last_lines(&term_with("", 20, 5, 0), 0), "");
    }

    #[test]
    fn environment_lines_are_filtered() {
        let parsed: Vec<_> = parse_environment("EDITOR=vim\n# comment\nBAD NAME=1\nA=b=c\n").collect();
        assert_eq!(
            parsed,
            [
                ("EDITOR".to_string(), "vim".to_string()),
                ("A".to_string(), "b=c".to_string())
            ]
        );
    }

    #[test]
    fn the_shell_gets_a_widely_known_terminal_type() {
        let environment = shell_environment("");
        assert_eq!(environment["TERM"], "xterm-256color");
        assert_eq!(environment["COLORTERM"], "truecolor");
        // No window of X11, nor of Alacritty, unless the user sets one.
        assert_eq!(environment["WINDOWID"], "");
        assert_eq!(environment["ALACRITTY_WINDOW_ID"], "");
        assert_eq!(shell_environment("WINDOWID=42")["WINDOWID"], "42");
    }

    #[test]
    fn a_centered_grid_starts_on_whole_pixels() {
        // One pixel left over on each axis.
        let (x, y) = centered(101.0, 61.0, 80.0, 40.0, (4.0, 1.0));
        assert_eq!((x, y), (10.0, 10.0));
        let geometry = crate::render::Geometry {
            origin_x: x,
            origin_y: y,
            cell_width: 8.0,
            cell_height: 16.0,
            ascent: 12.0,
            pixel_ratio: 1.0,
        };
        assert_eq!(geometry.cell_rect(3, 1, 1.0).x, geometry.baseline(3, 1).0);
        // With no room, the margins stay.
        assert_eq!(centered(50.0, 20.0, 80.0, 40.0, (4.0, 1.0)), (4.0, 1.0));
    }

    #[test]
    fn grid_matches_konsole_for_a_quad_pane() {
        assert_eq!(grid_size(574, 173, 8.0, 15.0), (68, 11));
    }

    #[test]
    fn the_narrowest_pane_has_the_columns_the_emulator_needs() {
        // A pane dragged to its least width of 40 pixels, with the scroll bar.
        assert_eq!(grid_size(40, 100, 9.0, 18.0).0, MIN_COLUMNS as u16);
        assert_eq!(grid_size(0, 100, 9.0, 18.0).0, MIN_COLUMNS as u16);
        assert_eq!(least_columns(1), 2);
        assert_eq!(least_columns(80), 80);
    }

    #[test]
    fn the_narrowest_grid_takes_any_output() {
        // As narrow as a pane gets: wide characters, tabs, combining marks, the insert mode,
        // deleting and inserting characters, and wrapping, each in a thread of its own, so
        // that a loop shows as a timeout rather than a test that never ends.
        let columns = usize::from(least_columns(1));
        for output in [
            "日本語\u{1f680}日本語\r\n",
            "\ta\tb\t\tc\r\n",
            "e\u{301}\u{302}\u{303}a\u{301}\r\n",
            "\x1b[4hab日cd\x1b[4l\r\n",
            "abc\x1b[1G\x1b[5P\x1b[5@日\x1b[3X\r\n",
            "\x1b[?7lwide日本語\x1b[?7h\r\n",
            "日\x1b[1D\x1b[1P\x1b[2;2H\x1b[1@本\r\n",
        ] {
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut term = term_with(&output.repeat(20), columns, 3, 100);
                term.resize(TermSize::new(columns, 2));
                let cells = term.renderable_content().display_iter.count();
                let _ = sender.send(cells);
            });
            let cells = receiver.recv_timeout(std::time::Duration::from_secs(2));
            assert_eq!(cells, Ok(columns * 2), "{output:?}");
        }
    }

    #[test]
    fn shrinking_with_a_wide_character_in_the_history_comes_back() {
        // An emoji in the prompt and CJK in the output, and the pane as narrow as it gets.
        let mut term = term_with("\u{1f680} ready\n日本語のテキスト\nascii\n$ ", 40, 3, 100);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            term.resize(TermSize::new(usize::from(least_columns(1)), 3));
            let _ = sender.send(term.columns());
        });
        let columns = receiver.recv_timeout(std::time::Duration::from_secs(2));
        assert_eq!(columns, Ok(MIN_COLUMNS), "the resize did not come back");
    }
}
