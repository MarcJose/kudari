// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The `io.github.marcjose.kudari` D-Bus interface.
//!
//! Method names and semantics match Yakuake's `org.kde.yakuake`, so scripts only
//! have to change the service, interface and object paths.

// `#[interface]` needs methods with `self`, but all state lives on the Qt thread.
#![allow(clippy::unused_self, clippy::used_underscore_binding)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use zbus::connection::Builder;
use zbus::{Connection, block_on, interface};

use crate::controller::{qobject::Controller, queue};
use crate::sessions::{Direction, Orientation, Session, SessionType, TerminalState};

pub const SERVICE: &str = "io.github.marcjose.kudari";

const POINTER_SCRIPT_NAME: &str = "kudari-pointer-output";
/// How long to wait for `KWin` before falling back to its active output.
const POINTER_TIMEOUT: Duration = Duration::from_millis(150);
/// Runs inside `KWin`, the only place that knows the pointer position on Wayland.
const POINTER_SCRIPT: &str = r#"callDBus("io.github.marcjose.kudari", "/kudari/window", "io.github.marcjose.kudari", "pointerOutput", workspace.screenAt(workspace.cursorPos).name);
"#;

static CONNECTION: OnceLock<Connection> = OnceLock::new();
/// Receiver of the next `pointerOutput` call.
static POINTER_REPLY: Mutex<Option<mpsc::Sender<String>>> = Mutex::new(None);

/// How long a D-Bus call waits for the Qt thread. The Qt thread can itself be waiting for
/// a reply on this connection, so an unlimited wait could block both for good.
const QT_TIMEOUT: Duration = Duration::from_secs(1);

/// Runs `action` on the Qt thread and waits for the result, or gives `fallback` when the Qt
/// thread did not start it within [`QT_TIMEOUT`]. Then it never runs, so a caller that hears
/// of a failure can try again without the first attempt happening as well.
///
/// The wait happens on a thread of its own. The thread of the connection stays free for
/// other calls and for the replies the Qt thread may be waiting for, such as the one of
/// `KWin` with the screen under the pointer; waiting there held up both for a second.
pub async fn ask<T: Send + 'static>(
    action: impl FnOnce(std::pin::Pin<&mut Controller>) -> T + Send + 'static,
    fallback: T,
) -> T {
    let (handoff, claim) = kudari_core::handoff::handoff();
    let (sender, receiver) = mpsc::channel();
    queue(move |controller| {
        if claim.start() {
            let _ = sender.send(action(controller));
        }
    });
    let reply = Reply::default();
    let answer = reply.clone();
    let waiting = std::thread::Builder::new()
        .name("kudari-ask".to_string())
        .spawn(move || answer.fill(handoff.wait(&receiver, QT_TIMEOUT)));
    if waiting.is_err() {
        return fallback;
    }
    reply.await.unwrap_or(fallback)
}

/// The answer that a thread hands to an `async` caller, once it is there.
struct Slot<T> {
    /// Whether the thread is done, with or without an answer.
    done: bool,
    answer: Option<T>,
    waker: Option<std::task::Waker>,
}

/// The answer that a thread hands to an `async` caller.
struct Reply<T>(std::sync::Arc<Mutex<Slot<T>>>);

impl<T> Default for Reply<T> {
    fn default() -> Self {
        Self(std::sync::Arc::new(Mutex::new(Slot {
            done: false,
            answer: None,
            waker: None,
        })))
    }
}

impl<T> Clone for Reply<T> {
    fn clone(&self) -> Self {
        Self(std::sync::Arc::clone(&self.0))
    }
}

impl<T> Reply<T> {
    fn fill(&self, answer: Option<T>) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.done = true;
        slot.answer = answer;
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

impl<T> std::future::Future for Reply<T> {
    type Output = Option<T>;

    fn poll(self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<Option<T>> {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.done {
            return std::task::Poll::Ready(slot.answer.take());
        }
        slot.waker = Some(context.waker().clone());
        std::task::Poll::Pending
    }
}

fn ids(values: impl IntoIterator<Item = i32>) -> String {
    values
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

struct Window;

#[interface(name = "io.github.marcjose.kudari")]
impl Window {
    #[zbus(name = "toggleWindowState")]
    fn toggle_window_state(&self) {
        queue(super::controller::qobject::Controller::toggle_window);
    }

    /// The reply of the `KWin` script started by [`pointer_output`].
    #[zbus(name = "pointerOutput")]
    fn pointer_output(&self, output: String) {
        let sender = POINTER_REPLY.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(output);
        }
    }
}

struct Tabs;

#[interface(name = "io.github.marcjose.kudari")]
impl Tabs {
    #[zbus(name = "tabTitle")]
    async fn tab_title(&self, session: i32) -> String {
        ask(
            move |c| c.stack().tab_title(session).unwrap_or_default().to_string(),
            String::new(),
        )
        .await
    }

    #[zbus(name = "setTabTitle")]
    fn set_tab_title(&self, session: i32, title: String) {
        queue(move |mut c| {
            c.as_mut().stack_mut().set_tab_title(session, &title, true);
            c.refresh();
        });
    }

    #[zbus(name = "sessionAtTab")]
    async fn session_at_tab(&self, index: i32) -> i32 {
        ask(move |c| c.stack().session_at_tab(index), -1).await
    }
}

struct Sessions;

async fn add(kind: SessionType) -> i32 {
    ask(move |c| c.add_session(kind), -1).await
}

/// Resolves `-1` to the active session, which scripts written for Yakuake rely on.
fn resolve(c: &Controller, session: i32) -> Option<i32> {
    if session == -1 {
        c.stack().active_id()
    } else {
        c.stack().session(session).map(|found| found.id)
    }
}

async fn session_flag(session: i32, read: impl FnOnce(&Session) -> bool + Send + 'static) -> bool {
    ask(
        move |c| {
            resolve(&c, session)
                .and_then(|id| c.stack().session(id))
                .is_some_and(read)
        },
        false,
    )
    .await
}

/// Typing into terminals over D-Bus runs code as the user, so it needs an explicit opt-in.
async fn require_remote_commands() -> zbus::fdo::Result<()> {
    if ask(|c| c.allows_remote_commands(), false).await {
        Ok(())
    } else {
        Err(zbus::fdo::Error::AccessDenied(
            "Remote commands are disabled in the settings".to_string(),
        ))
    }
}

async fn terminal_flag(terminal: i32, read: impl FnOnce(&TerminalState) -> bool + Send + 'static) -> bool {
    ask(move |c| c.stack().terminal_state(terminal).is_some_and(read), false).await
}

fn set_session_flag(session: i32, kind: &'static str, checked: bool) {
    queue(move |c| {
        if let Some(id) = resolve(&c, session) {
            c.set_all_monitor(id, kind, checked);
        }
    });
}

fn all_terminals(session: &Session, test: impl Fn(&TerminalState) -> bool) -> bool {
    session.terminals.values().all(test)
}

fn any_terminal(session: &Session, test: impl Fn(&TerminalState) -> bool) -> bool {
    session.terminals.values().any(test)
}

async fn grow(terminal: i32, direction: Direction, pixels: u32) -> i32 {
    ask(
        move |mut c| {
            let (width, height) = c.area();
            let grown = c
                .as_mut()
                .stack_mut()
                .try_grow(terminal, direction, pixels as i32, width, height);
            c.refresh();
            grown
        },
        -1,
    )
    .await
}

/// Splits through the app, so that the new terminal starts where the one split is.
async fn split_terminal(terminal: i32, orientation: Orientation) -> i32 {
    ask(move |c| c.split_terminal(terminal, orientation), -1).await
}

async fn split_session(session: i32, orientation: Orientation) -> i32 {
    ask(move |c| c.split(session, Some(orientation)), -1).await
}

#[interface(name = "io.github.marcjose.kudari")]
impl Sessions {
    #[zbus(name = "addSession")]
    async fn add_session(&self) -> i32 {
        add(SessionType::Single).await
    }

    #[zbus(name = "addSessionTwoHorizontal")]
    async fn add_session_two_horizontal(&self) -> i32 {
        add(SessionType::TwoHorizontal).await
    }

    #[zbus(name = "addSessionTwoVertical")]
    async fn add_session_two_vertical(&self) -> i32 {
        add(SessionType::TwoVertical).await
    }

    #[zbus(name = "addSessionQuad")]
    async fn add_session_quad(&self) -> i32 {
        add(SessionType::Quad).await
    }

    #[zbus(name = "raiseSession")]
    fn raise_session(&self, session: i32) {
        queue(move |c| c.raise(session));
    }

    #[zbus(name = "removeSession")]
    fn remove_session(&self, session: i32) {
        queue(move |c| {
            if let Some(session) = resolve(&c, session) {
                c.remove_session(session);
            }
        });
    }

    #[zbus(name = "removeTerminal")]
    fn remove_terminal(&self, terminal: i32) {
        queue(move |c| {
            let target = if terminal == -1 {
                c.stack().active().map(|session| session.active_terminal)
            } else {
                Some(terminal)
            };
            let closable = target
                .and_then(|id| c.stack().session_for_terminal(id))
                .and_then(|id| c.stack().session(id))
                .is_some_and(|session| session.closable);
            if let (Some(target), true) = (target, closable) {
                c.close_terminal(target);
            }
        });
    }

    #[zbus(name = "splitSessionAuto")]
    async fn split_session_auto(&self, session: i32) -> i32 {
        ask(move |c| c.split(session, None), -1).await
    }

    #[zbus(name = "splitSessionLeftRight")]
    async fn split_session_left_right(&self, session: i32) -> i32 {
        split_session(session, Orientation::Horizontal).await
    }

    #[zbus(name = "splitSessionTopBottom")]
    async fn split_session_top_bottom(&self, session: i32) -> i32 {
        split_session(session, Orientation::Vertical).await
    }

    #[zbus(name = "splitTerminalLeftRight")]
    async fn split_terminal_left_right(&self, terminal: i32) -> i32 {
        split_terminal(terminal, Orientation::Horizontal).await
    }

    #[zbus(name = "splitTerminalTopBottom")]
    async fn split_terminal_top_bottom(&self, terminal: i32) -> i32 {
        split_terminal(terminal, Orientation::Vertical).await
    }

    #[zbus(name = "tryGrowTerminalRight")]
    async fn try_grow_terminal_right(&self, terminal: i32, pixels: u32) -> i32 {
        grow(terminal, Direction::Right, pixels).await
    }

    #[zbus(name = "tryGrowTerminalLeft")]
    async fn try_grow_terminal_left(&self, terminal: i32, pixels: u32) -> i32 {
        grow(terminal, Direction::Left, pixels).await
    }

    #[zbus(name = "tryGrowTerminalTop")]
    async fn try_grow_terminal_top(&self, terminal: i32, pixels: u32) -> i32 {
        grow(terminal, Direction::Up, pixels).await
    }

    #[zbus(name = "tryGrowTerminalBottom")]
    async fn try_grow_terminal_bottom(&self, terminal: i32, pixels: u32) -> i32 {
        grow(terminal, Direction::Down, pixels).await
    }

    #[zbus(name = "activeSessionId")]
    async fn active_session_id(&self) -> i32 {
        ask(|c| c.stack().active_id().unwrap_or(-1), -1).await
    }

    #[zbus(name = "activeTerminalId")]
    async fn active_terminal_id(&self) -> i32 {
        ask(|c| c.stack().active().map_or(-1, |s| s.active_terminal), -1).await
    }

    #[zbus(name = "sessionIdList")]
    async fn session_id_list(&self) -> String {
        ask(|c| ids(c.stack().sessions().map(|s| s.id)), String::new()).await
    }

    #[zbus(name = "terminalIdList")]
    async fn terminal_id_list(&self) -> String {
        ask(|c| ids(c.stack().all_terminals()), String::new()).await
    }

    #[zbus(name = "terminalIdsForSessionId")]
    async fn terminal_ids_for_session_id(&self, session: i32) -> String {
        ask(
            move |c| {
                c.stack()
                    .session(session)
                    .map_or_else(|| "-1".to_string(), |s| ids(s.terminal_ids()))
            },
            "-1".to_string(),
        )
        .await
    }

    #[zbus(name = "sessionIdForTerminalId")]
    async fn session_id_for_terminal_id(&self, terminal: i32) -> i32 {
        ask(move |c| c.stack().session_for_terminal(terminal).unwrap_or(-1), -1).await
    }

    #[zbus(name = "runCommand")]
    async fn run_command(&self, command: String) -> zbus::fdo::Result<()> {
        require_remote_commands().await?;
        queue(move |c| {
            if let Some(terminal) = c.stack().active().map(|s| s.active_terminal) {
                c.run_command(terminal, &command);
            }
        });
        Ok(())
    }

    #[zbus(name = "runCommandInTerminal")]
    async fn run_command_in_terminal(&self, terminal: i32, command: String) -> zbus::fdo::Result<()> {
        require_remote_commands().await?;
        queue(move |c| c.run_command(terminal, &command));
        Ok(())
    }

    #[zbus(name = "isSessionClosable")]
    async fn is_session_closable(&self, session: i32) -> bool {
        session_flag(session, |s| s.closable).await
    }

    #[zbus(name = "setSessionClosable")]
    fn set_session_closable(&self, session: i32, closable: bool) {
        queue(move |mut c| {
            if let Some(id) = resolve(&c, session) {
                if let Some(found) = c.as_mut().stack_mut().session_mut(id) {
                    found.closable = closable;
                }
                c.refresh();
            }
        });
    }

    #[zbus(name = "hasUnclosableSessions")]
    async fn has_unclosable_sessions(&self) -> bool {
        ask(|c| c.stack().has_unclosable_sessions(), false).await
    }

    #[zbus(name = "isSessionKeyboardInputEnabled")]
    async fn is_session_keyboard_input_enabled(&self, session: i32) -> bool {
        session_flag(session, |s| all_terminals(s, |t| t.keyboard_input_enabled)).await
    }

    #[zbus(name = "setSessionKeyboardInputEnabled")]
    fn set_session_keyboard_input_enabled(&self, session: i32, enabled: bool) {
        set_session_flag(session, "keyboard", !enabled);
    }

    #[zbus(name = "isTerminalKeyboardInputEnabled")]
    async fn is_terminal_keyboard_input_enabled(&self, terminal: i32) -> bool {
        terminal_flag(terminal, |t| t.keyboard_input_enabled).await
    }

    #[zbus(name = "setTerminalKeyboardInputEnabled")]
    fn set_terminal_keyboard_input_enabled(&self, terminal: i32, enabled: bool) {
        queue(move |c| c.terminal_toggled("keyboard", terminal, !enabled));
    }

    #[zbus(name = "isSessionMonitorActivityEnabled")]
    async fn is_session_monitor_activity_enabled(&self, session: i32) -> bool {
        session_flag(session, |s| all_terminals(s, |t| t.monitor_activity)).await
    }

    #[zbus(name = "setSessionMonitorActivityEnabled")]
    fn set_session_monitor_activity_enabled(&self, session: i32, enabled: bool) {
        set_session_flag(session, "activity", enabled);
    }

    #[zbus(name = "isTerminalMonitorActivityEnabled")]
    async fn is_terminal_monitor_activity_enabled(&self, terminal: i32) -> bool {
        terminal_flag(terminal, |t| t.monitor_activity).await
    }

    #[zbus(name = "setTerminalMonitorActivityEnabled")]
    fn set_terminal_monitor_activity_enabled(&self, terminal: i32, enabled: bool) {
        queue(move |c| c.terminal_toggled("activity", terminal, enabled));
    }

    #[zbus(name = "hasTerminalsWithMonitorActivityEnabled")]
    async fn has_terminals_with_monitor_activity_enabled(&self, session: i32) -> bool {
        session_flag(session, |s| any_terminal(s, |t| t.monitor_activity)).await
    }

    #[zbus(name = "hasTerminalsWithMonitorActivityDisabled")]
    async fn has_terminals_with_monitor_activity_disabled(&self, session: i32) -> bool {
        session_flag(session, |s| any_terminal(s, |t| !t.monitor_activity)).await
    }

    #[zbus(name = "isSessionMonitorSilenceEnabled")]
    async fn is_session_monitor_silence_enabled(&self, session: i32) -> bool {
        session_flag(session, |s| all_terminals(s, |t| t.monitor_silence)).await
    }

    #[zbus(name = "setSessionMonitorSilenceEnabled")]
    fn set_session_monitor_silence_enabled(&self, session: i32, enabled: bool) {
        set_session_flag(session, "silence", enabled);
    }

    #[zbus(name = "isTerminalMonitorSilenceEnabled")]
    async fn is_terminal_monitor_silence_enabled(&self, terminal: i32) -> bool {
        terminal_flag(terminal, |t| t.monitor_silence).await
    }

    #[zbus(name = "setTerminalMonitorSilenceEnabled")]
    fn set_terminal_monitor_silence_enabled(&self, terminal: i32, enabled: bool) {
        queue(move |c| c.terminal_toggled("silence", terminal, enabled));
    }

    #[zbus(name = "hasTerminalsWithMonitorSilenceEnabled")]
    async fn has_terminals_with_monitor_silence_enabled(&self, session: i32) -> bool {
        session_flag(session, |s| any_terminal(s, |t| t.monitor_silence)).await
    }

    #[zbus(name = "hasTerminalsWithMonitorSilenceDisabled")]
    async fn has_terminals_with_monitor_silence_disabled(&self, session: i32) -> bool {
        session_flag(session, |s| any_terminal(s, |t| !t.monitor_silence)).await
    }

    #[zbus(name = "hasTerminalsWithKeyboardInputEnabled")]
    async fn has_terminals_with_keyboard_input_enabled(&self, session: i32) -> bool {
        session_flag(session, super::sessions::Session::has_keyboard_input_enabled).await
    }

    #[zbus(name = "hasTerminalsWithKeyboardInputDisabled")]
    async fn has_terminals_with_keyboard_input_disabled(&self, session: i32) -> bool {
        session_flag(session, super::sessions::Session::has_keyboard_input_disabled).await
    }
}

/// Drives the UI from automated tests; only present with the `test-hooks` feature.
#[cfg(feature = "test-hooks")]
struct TestHooks;

/// See [`TestHooks::settle`]; on a thread of its own, as it waits.
#[cfg(feature = "test-hooks")]
fn settle_within(quiet: Duration, most: Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        // A trip through the Qt thread: what was queued before has run, and it reads the frames.
        let (sender, receiver) = mpsc::channel();
        queue(move |_| {
            let _ = sender.send(crate::kde::rk_test_quiet_ms());
        });
        let frames = receiver
            .recv_timeout(QT_TIMEOUT)
            .ok()
            .map(|milliseconds| Duration::from_millis(u64::try_from(milliseconds).unwrap_or(0)));
        let calm = [Some(started.elapsed()), frames, kudari_core::stats::quiet()]
            .into_iter()
            .try_fold(Duration::MAX, |least, time| time.map(|time| least.min(time)));
        if calm.is_some_and(|calm| calm >= quiet) {
            return true;
        }
        if started.elapsed() >= most {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "test-hooks")]
#[interface(name = "io.github.marcjose.kudari.Test")]
impl TestHooks {
    fn key(&self, sequence: String) {
        queue(move |_| crate::kde::rk_test_key(&crate::kde::qs(&sequence)));
    }

    fn input_method(&self, preedit: String, commit: String) {
        queue(move |_| crate::kde::rk_test_input_method(&crate::kde::qs(&preedit), &crate::kde::qs(&commit)));
    }

    /// The counters of what Kudari does, as JSON; see `kudari_core::stats`. They are atomics,
    /// read on this thread without the Qt thread.
    fn stats(&self) -> String {
        kudari_core::stats::json()
    }

    /// Sets the counters back, so that a check counts from now on.
    fn reset_stats(&self) {
        kudari_core::stats::reset();
    }

    /// Whether Kudari has started: its first session is there and, if the settings say so,
    /// its window.
    fn started(&self) -> bool {
        crate::controller::started()
    }

    /// Waits until the user interface has settled: nothing waits for the Qt thread, and
    /// neither a terminal nor a window of Qt Quick has drawn for `quiet` milliseconds since
    /// this call. False when that did not come within `most` milliseconds, at most 20 s.
    async fn settle(&self, quiet: u32, most: u32) -> bool {
        let quiet = Duration::from_millis(quiet.into());
        let most = Duration::from_millis(most.min(20_000).into());
        let reply = Reply::default();
        let answer = reply.clone();
        let waiting = std::thread::Builder::new()
            .name("kudari-settle".to_string())
            .spawn(move || answer.fill(Some(settle_within(quiet, most))));
        if waiting.is_err() {
            return false;
        }
        reply.await.unwrap_or(false)
    }

    /// Paints each terminal that shows anew into `<path>-<terminal>.png`, and again as the
    /// reference, with a text drawn on its own for each cell, into `<path>-reference-…`; and
    /// saves what the window shows in its place into `<path>-<terminal>-shown.png`.
    fn paint_terminals(&self, path: String) {
        queue(move |_| {
            kudari_qt::terminal_item::set_reference_drawing(true);
            kudari_qt::qt::rk_test_paint_terminals(&crate::kde::qs(&format!("{path}-reference")));
            kudari_qt::terminal_item::set_reference_drawing(false);
            kudari_qt::qt::rk_test_paint_terminals(&crate::kde::qs(&path));
        });
    }

    /// Lets each look at the file system for a link wait `milliseconds` first, as at a slow
    /// drive; 0 ends that.
    fn slow_file_looks(&self, milliseconds: u32) {
        kudari_core::file_looks::slow_down(milliseconds.into());
    }

    /// Lets the next frame a terminal paints panic, which breaks only that terminal.
    fn panic_next_paint(&self) {
        kudari_qt::terminal_item::panic_next_paint();
    }

    /// Lets each refresh of a terminal wait `milliseconds` at its end, so that output comes
    /// between a refresh and the paint of its frame; 0 ends that.
    fn slow_refresh(&self, milliseconds: u32) {
        kudari_qt::terminal_item::set_refresh_delay(milliseconds.into());
    }

    /// The text under each match of the search that the last frame of the terminal showed,
    /// a line each.
    fn matched_text(&self, terminal: i32) -> String {
        kudari_qt::terminal_item::matched_text(terminal)
    }

    /// Saves an image of the window, to find where the checks have to click.
    fn grab(&self, path: String) {
        queue(move |_| crate::kde::rk_test_grab(&crate::kde::qs(&path)));
    }

    /// A property of the objects of QML with this `objectName`, with commas between them.
    async fn property(&self, name: String, property: String) -> String {
        ask(
            move |_| crate::kde::rk_test_property(&crate::kde::qs(&name), &crate::kde::qs(&property)).to_string(),
            String::new(),
        )
        .await
    }

    async fn state(&self) -> String {
        ask(|_| crate::kde::rk_test_window_state().to_string(), String::new()).await
    }

    /// The background of the terminals, as `#rrggbb`.
    async fn terminal_background(&self) -> String {
        ask(
            |_| {
                let color = kudari_core::colors::active().background;
                format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
            },
            String::new(),
        )
        .await
    }

    fn trigger(&self, action: String) {
        queue(move |c| c.trigger_action(&action, true));
    }

    /// Triggers a checkable action as if it had been switched to `checked`.
    fn toggle(&self, action: String, checked: bool) {
        queue(move |c| c.trigger_action(&action, checked));
    }

    fn menu_value(&self, kind: String, value: i32) {
        queue(move |c| c.menu_value(&kind, value));
    }

    fn save_output(&self, terminal: i32, path: String) {
        queue(move |_| {
            if let Some(backend) = crate::terminal::backend(terminal) {
                let _ = backend.save_output(std::path::Path::new(&path));
            }
        });
    }

    fn set_setting(&self, key: String, value: String) {
        queue(move |c| c.set_setting_from_test(&key, &value));
    }

    /// The text of the open question about an AI agent, or nothing.
    async fn agent_question(&self) -> String {
        ask(|_| crate::kde::rk_test_agent_question().to_string(), String::new()).await
    }

    /// Clicks Allow or Deny in the question about an AI agent.
    fn answer_agent(&self, allow: bool) {
        queue(move |_| crate::kde::rk_test_answer_agent(allow));
    }

    /// Whether the window stays below an open dialog of Kudari.
    async fn dialog_open(&self) -> bool {
        ask(|c| *c.dialog_open(), false).await
    }

    /// Whether the bar shows that an AI agent has access.
    async fn agent_access(&self) -> bool {
        ask(|c| *c.agent_access(), false).await
    }

    /// Like a click on the indicator in the bar.
    fn revoke_agents(&self) {
        queue(Controller::revoke_agent_access);
    }

    /// Drops text on the active terminal, as from another program.
    fn drop_text(&self, text: String) {
        queue(move |c| {
            if let Some(terminal) = c.stack().active().map(|session| session.active_terminal) {
                kudari_qt::terminal_item::drop_into(terminal, text, false);
            }
        });
    }

    fn popup(&self, menu: String) {
        queue(move |c| match menu.as_str() {
            "main" => crate::kde::rk_popup_main_menu(1000, 400),
            "session" => crate::kde::rk_popup_session_menu(40, 370),
            drop if drop.starts_with('/') => c.popup_drop_active(drop),
            _ => c.test_popup_tab_menu(),
        });
    }
}

struct Application;

/// Lets `.desktop` activation and `kudari` itself toggle the running window.
#[interface(name = "org.freedesktop.Application")]
impl Application {
    fn activate(&self, _platform_data: std::collections::HashMap<String, zbus::OwnedVariant>) {
        // Started from the menu, the bus activates Kudari before it has started; that asks
        // for the window, which a toggle after a start that opens it would close again.
        if super::controller::started() {
            queue(super::controller::qobject::Controller::toggle_window);
        } else {
            queue(super::controller::qobject::Controller::show_window);
        }
    }
}

/// The file of the script, written once a run of Kudari, and again where it went: a script of
/// another version can lie there.
fn pointer_script_path() -> Option<PathBuf> {
    static WRITTEN: AtomicBool = AtomicBool::new(false);
    let path = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("kudari-pointer-output.js");
    if !WRITTEN.load(Ordering::Relaxed) || !path.exists() {
        std::fs::write(&path, POINTER_SCRIPT).ok()?;
        WRITTEN.store(true, Ordering::Relaxed);
    }
    Some(path)
}

fn unload_pointer_script(connection: &Connection) {
    let _ = block_on(connection.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "unloadScript",
        &POINTER_SCRIPT_NAME,
    ));
}

/// [`unload_pointer_script`] without waiting for the answer: `KWin` takes the calls of a
/// connection in their order, so the next load comes after it.
fn unload_pointer_script_later(connection: &Connection) {
    let message = zbus::message::Message::method_call("/Scripting", "unloadScript")
        .destination("org.kde.KWin")
        .interface("org.kde.kwin.Scripting")
        .with_flags(zbus::message::Flags::NO_REPLY_EXPECTED)
        .build(&POINTER_SCRIPT_NAME);
    if let Ok(message) = message {
        let _ = block_on(connection.send(&message));
    }
}

/// The output under the mouse pointer.
///
/// `activeOutputName` is no replacement: closing the window hands the focus back to
/// another window, which moves `KWin`'s active output there while the pointer stays put.
/// Blocks for at most [`POINTER_TIMEOUT`] plus two round trips to `KWin`: loading the script
/// and starting it. A script that a Kudari before left is unloaded the first time; after
/// that, each unloads itself, as Kudari tells `KWin` without waiting.
pub fn pointer_output() -> Option<String> {
    static UNLOADED: std::sync::Once = std::sync::Once::new();
    let connection = CONNECTION.get()?;
    let path = pointer_script_path()?;
    UNLOADED.call_once(|| unload_pointer_script(connection));

    let (sender, receiver) = mpsc::channel();
    *POINTER_REPLY.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);

    let script = block_on(connection.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "loadScript",
        &(path.to_string_lossy().as_ref(), POINTER_SCRIPT_NAME),
    ))
    .ok()?
    .body()
    .deserialize::<i32>()
    .ok()?;
    let script_path = format!("/Scripting/Script{script}");
    let started = block_on(connection.call_method(
        Some("org.kde.KWin"),
        script_path.as_str(),
        Some("org.kde.kwin.Script"),
        "run",
        &(),
    ));
    let output = started.ok().and_then(|_| receiver.recv_timeout(POINTER_TIMEOUT).ok());
    unload_pointer_script_later(connection);
    output
}

/// The process behind a connection to the bus.
pub fn process_id(name: &str) -> Option<u32> {
    let connection = CONNECTION.get()?;
    block_on(async {
        let bus = zbus::fdo::DBusProxy::new(connection).await.ok()?;
        let name = zbus::names::BusName::try_from(name).ok()?;
        bus.get_connection_unix_process_id(name).await.ok()
    })
}

/// Tells the agent interface about every connection that leaves the bus, so the access of
/// an agent ends with it.
async fn watch_disconnects(connection: Connection) -> zbus::Result<()> {
    use zbus::export::futures_core::Stream;
    let bus = zbus::fdo::DBusProxy::new(&connection).await?;
    let changes = bus.receive_name_owner_changed().await?;
    let mut changes = std::pin::pin!(changes);
    while let Some(change) = std::future::poll_fn(|context| changes.as_mut().poll_next(context)).await {
        let Ok(arguments) = change.args() else {
            continue;
        };
        let name = arguments.name().to_string();
        if name.starts_with(':') && arguments.new_owner().is_none() {
            queue(move |c| c.agent_gone(&name));
        }
    }
    Ok(())
}

/// Owns the service name. Returns `None` when another instance already runs.
pub fn serve() -> zbus::Result<Option<Connection>> {
    let builder = Builder::session()
        .serve_at("/kudari/window", Window)
        .serve_at("/kudari/tabs", Tabs)
        .serve_at("/kudari/sessions", Sessions)
        .serve_at("/kudari/agent", crate::agent::Agent)
        .serve_at("/io/github/marcjose/kudari", Application);
    #[cfg(feature = "test-hooks")]
    let builder = builder.serve_at("/kudari/test", TestHooks);
    let connection = block_on(builder.build())?;
    match block_on(connection.request_name_with_flags(SERVICE, zbus::fdo::RequestNameFlags::DO_NOT_QUEUE)) {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner) => {
            let _ = CONNECTION.set(connection.clone());
            let watched = connection.clone();
            let watching = std::thread::Builder::new()
                .name("agents gone".to_string())
                .spawn(move || block_on(watch_disconnects(watched)));
            if let Err(error) = watching {
                eprintln!("kudari: agents that go are not noticed: {error}");
            }
            Ok(Some(connection))
        }
        Ok(_) | Err(zbus::Error::NameTaken) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Toggles the instance that owns the service.
pub fn toggle_running_instance() -> zbus::Result<()> {
    block_on(async {
        let connection = Connection::session().await?;
        connection
            .call_method(
                Some(SERVICE),
                "/kudari/window",
                Some("io.github.marcjose.kudari"),
                "toggleWindowState",
                &(),
            )
            .await?;
        Ok(())
    })
}
