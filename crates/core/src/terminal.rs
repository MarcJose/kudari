// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A running shell with its emulator state. The front end hears from the terminals through
//! a [`Listener`], which it installs with [`set_listener`].

use std::collections::HashMap;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{CursorStyle, Handler, Processor, StdSyncHandler};

use crate::command::Quoting;
use crate::marks::{Marks, Prompt, Tee};
#[cfg(windows)]
use crate::process::Console;
#[cfg(unix)]
use crate::process::Pty;
use crate::process::{self, Programs, Signal};
use crate::services::{Clipboard, Desktop, Selection};
use crate::sessions::TerminalId;
use crate::title;

pub use crate::options::Options;

/// What a terminal tells its front end.
#[derive(Debug)]
pub enum Report {
    /// New output to show. More output is reported only once this is dropped, so the front
    /// end drops it right before it reads the terminal: output that arrives meanwhile is
    /// reported anew.
    Output(Pending),
    Bell,
    /// The shell ended.
    Exited,
}

/// Holds back further reports of output while it lives.
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
    *OPTIONS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(options);
    for backend in backends().values() {
        backend.apply_config();
        backend.apply_flow_control();
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
        (
            (width - columns).max(2.0 * x) / 2.0,
            (height - lines).max(2.0 * y) / 2.0,
        )
    } else {
        (x, y)
    }
}

/// Columns and lines of a terminal pane of the given pixel size. The shell has to
/// start with this size, or it would redraw its prompt on the first resize.
#[must_use]
pub fn grid_size(width: i32, height: i32, cell_width: f64, cell_height: f64) -> (u16, u16) {
    let (margin_x, margin_y) = text_margin();
    let columns = (f64::from(width) - scrollbar_space() - 2.0 * margin_x) / cell_width;
    let lines = (f64::from(height) - 2.0 * margin_y) / cell_height;
    (
        (columns as i32).clamp(1, 4096) as u16,
        (lines as i32).clamp(1, 4096) as u16,
    )
}

#[derive(Clone)]
pub struct Proxy {
    id: TerminalId,
    repaint_queued: Arc<AtomicBool>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let id = self.id;
        let Some(listener) = listener() else {
            return;
        };
        match event {
            Event::Wakeup if !self.repaint_queued.swap(true, Ordering::AcqRel) => {
                listener.report(id, Report::Output(Pending(self.repaint_queued.clone())));
            }
            Event::Title(title) => run(move || {
                if let Some(backend) = backend(id) {
                    *backend
                        .program_title
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = title;
                }
            }),
            Event::ResetTitle => run(move || {
                if let Some(backend) = backend(id) {
                    backend
                        .program_title
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clear();
                }
            }),
            Event::PtyWrite(text) => run(move || {
                if let Some(backend) = backend(id) {
                    backend.write(text.into_bytes());
                }
            }),
            Event::Bell => listener.report(id, Report::Bell),
            Event::ClipboardStore(_, text) => run(move || listener.clipboard().copy(&text, Selection::Clipboard)),
            Event::ClipboardLoad(_, format) => run(move || {
                let text = listener.clipboard().paste(Selection::Clipboard);
                if let Some(backend) = backend(id) {
                    backend.write(format(&text).into_bytes());
                }
            }),
            Event::ChildExit(_) | Event::Exit => listener.report(id, Report::Exited),
            _ => {}
        }
    }
}

/// What the operating system tells about the programs of a terminal.
#[cfg(unix)]
type System = Pty;
#[cfg(windows)]
type System = Console;

pub struct Backend {
    pub term: Arc<FairMutex<Term<Proxy>>>,
    notifier: Notifier,
    /// The shell and what it runs.
    programs: Arc<System>,
    /// What the shell said about itself in its output.
    marks: Arc<Mutex<Marks>>,
    pub last_output: Mutex<Instant>,
    /// Title set by the running program.
    program_title: Mutex<String>,
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
    fn spawn(
        id: TerminalId,
        columns: u16,
        lines: u16,
        cell_width: u16,
        cell_height: u16,
        working_directory: Option<PathBuf>,
        shell: Option<&str>,
    ) -> Result<Arc<Self>, String> {
        let proxy = Proxy {
            id,
            repaint_queued: Arc::default(),
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
        let programs = Pty::new(pty.file().as_raw_fd(), pty.child().id() as i32);
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
        event_loop.spawn();

        let backend = Arc::new(Self {
            term,
            notifier,
            programs,
            marks,
            last_output: Mutex::new(Instant::now()),
            program_title: Mutex::new(String::new()),
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
        self.term.lock().set_options(config);
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

    /// Input always returns the view to the bottom of the scrollback.
    pub fn write_user_input(&self, bytes: Vec<u8>) {
        self.term.lock().scroll_display(Scroll::Bottom);
        self.write(bytes);
    }

    pub fn resize(&self, columns: u16, lines: u16, cell_width: u16, cell_height: u16) {
        let mut term = self.term.lock();
        if term.columns() == usize::from(columns) && term.screen_lines() == usize::from(lines) {
            return;
        }
        term.resize(TermSize::new(columns.into(), lines.into()));
        drop(term);
        let mut notifier = Notifier(self.notifier.0.clone());
        notifier.on_resize(WindowSize {
            num_lines: lines,
            num_cols: columns,
            cell_width,
            cell_height,
        });
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
        self.program_title
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The title of the terminal: the remote format while `ssh` runs, else `format`.
    pub fn title(&self, format: &str) -> String {
        let remote = options().map(|options| options.remote_title_format).unwrap_or_default();
        title::title(
            &*self.programs,
            self.working_directory(),
            format,
            &remote,
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
        let term = self.term.lock();
        let start = Point::new(term.grid().topmost_line(), Column(0));
        let end = Point::new(term.grid().bottommost_line(), term.grid().last_column());
        term.bounds_to_string(start, end)
    }

    /// The last `lines` lines of scrollback and screen, or the screen when `lines` is zero.
    pub fn recent_text(&self, lines: usize) -> String {
        last_lines(&self.term.lock(), lines)
    }

    /// Columns and lines of the screen.
    pub fn size(&self) -> (usize, usize) {
        let term = self.term.lock();
        (term.columns(), term.screen_lines())
    }

    /// Whether the program asked for the cursor keys of the application mode.
    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether the program asked for bracketed paste, as line editors such as those of
    /// bash, zsh and fish do while they edit a command line, and only then.
    pub fn bracketed_paste(&self) -> bool {
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Forgets the scrollback and returns to the initial terminal state.
    pub fn reset(&self) {
        self.term.lock().reset_state();
        // Ctrl+L makes the shell draw its prompt again.
        self.write(vec![0x0c]);
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

/// What AI agents see of a terminal.
impl crate::agent::Terminal for Backend {
    fn size(&self) -> (usize, usize) {
        Backend::size(self)
    }

    fn title(&self, format: &str) -> String {
        Backend::title(self, format)
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
    }

    #[test]
    fn grid_matches_konsole_for_a_quad_pane() {
        assert_eq!(grid_size(574, 173, 8.0, 15.0), (68, 11));
    }
}
