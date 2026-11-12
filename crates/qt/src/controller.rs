// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The application object for QML: carries what QML and KDE report to the app of the core,
//! and carries out the effects the app returns.

#![allow(
    clippy::must_use_candidate,
    reason = "cxx-qt generates public functions without these attributes"
)]
// Invokables need `self` for cxx-qt, also when they only call into Qt.
#![allow(clippy::unused_self)]

use std::cell::Cell;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};

use crate::app::{self, App, Effect, Host, Monitor, TerminalCommand};
use crate::colors;
use crate::frontend::frontend;
use crate::qt::{self, qs};
use crate::services;
use crate::sessions::{Layout, Orientation, SessionId, SessionStack, SessionType, TerminalId};
use crate::settings::{self, Settings};
use crate::terminal;
use crate::theme;
use kudari_core::i18n::tr;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, skin_json, cxx_name = "skinJson")]
        #[qproperty(QString, tabs_json, cxx_name = "tabsJson")]
        #[qproperty(QString, layout_json, cxx_name = "layoutJson")]
        #[qproperty(QString, terminal_ids_json, cxx_name = "terminalIdsJson")]
        #[qproperty(QString, settings_json, cxx_name = "settingsJson")]
        #[qproperty(QString, shortcuts_json, cxx_name = "shortcutsJson")]
        #[qproperty(QString, title)]
        #[qproperty(QString, fonts_json, cxx_name = "fontsJson")]
        #[qproperty(QString, highlight_color, cxx_name = "highlightColor")]
        #[qproperty(QString, output_name, cxx_name = "outputName")]
        #[qproperty(i32, work_width, cxx_name = "workWidth")]
        #[qproperty(i32, work_height, cxx_name = "workHeight")]
        #[qproperty(bool, window_visible, cxx_name = "windowVisible")]
        #[qproperty(bool, fullscreen)]
        #[qproperty(bool, dialog_open, cxx_name = "dialogOpen")]
        /// True while an AI agent may use Kudari.
        #[qproperty(bool, agent_access, cxx_name = "agentAccess")]
        type Controller = super::ControllerRust;

        /// Must run once after QML is ready: starts the first session.
        #[qinvokable]
        fn start(self: Pin<&mut Self>);

        /// Reports the area for terminals and the terminal cell size.
        #[qinvokable]
        fn set_geometry(self: Pin<&mut Self>, width: i32, height: i32, cell_width: f64, cell_height: f64);

        #[qinvokable]
        fn trigger(self: Pin<&mut Self>, name: &QString);

        #[qinvokable]
        fn poll(self: Pin<&mut Self>);

        #[qinvokable]
        fn select_tab(self: Pin<&mut Self>, session: i32);

        #[qinvokable]
        fn add_session_action(self: Pin<&mut Self>);

        #[qinvokable]
        fn close_tab(self: Pin<&mut Self>, session: i32);

        #[qinvokable]
        fn close_active_tab(self: Pin<&mut Self>);

        #[qinvokable]
        fn select_next_tab(self: Pin<&mut Self>);

        #[qinvokable]
        fn select_previous_tab(self: Pin<&mut Self>);

        #[qinvokable]
        fn drop_tab(self: Pin<&mut Self>, session: i32, target: i32);

        #[qinvokable]
        fn rename_tab(self: Pin<&mut Self>, session: i32, title: &QString);

        #[qinvokable]
        fn activate_terminal(self: Pin<&mut Self>, terminal: i32);

        /// Changes the font size of one terminal by `delta` points.
        #[qinvokable]
        fn zoom_terminal(self: Pin<&mut Self>, terminal: i32, delta: i32);

        #[qinvokable]
        fn drag_handle(self: Pin<&mut Self>, handle: i32, delta: i32);

        #[qinvokable]
        fn set_keep_open(self: Pin<&mut Self>, keep_open: bool);

        #[qinvokable]
        fn set_height_percent(self: Pin<&mut Self>, height: i32);

        #[qinvokable]
        fn focus_lost(self: Pin<&mut Self>);

        /// True while a menu or dialog of this application has the focus.
        #[qinvokable]
        fn application_has_focus(self: &Self) -> bool;

        #[qinvokable]
        fn popup_main_menu(self: Pin<&mut Self>, x: i32, y: i32);

        #[qinvokable]
        fn popup_session_menu(self: Pin<&mut Self>, x: i32, y: i32);

        #[qinvokable]
        fn popup_tab_menu(self: Pin<&mut Self>, session: i32, x: i32, y: i32);

        #[qinvokable]
        fn popup_terminal_menu(self: Pin<&mut Self>, x: i32, y: i32, can_copy: bool, link: &QString);

        /// A menu of `kudari_core::menus` for the QML of a front end without menus of its
        /// own, to show at `x`, `y`.
        #[qsignal]
        fn menu_requested(self: Pin<&mut Self>, x: i32, y: i32, menu: &QString);

        /// Switches a checkable action of a menu.
        #[qinvokable]
        fn toggle(self: Pin<&mut Self>, name: &QString, checked: bool);

        /// Chooses the value of an entry of a menu, as of the width of the window.
        #[qinvokable]
        fn choose(self: Pin<&mut Self>, kind: &QString, value: i32);

        /// Opens or copies the link of the terminal menu.
        #[qinvokable]
        fn use_link(self: Pin<&mut Self>, link: &QString, copy: bool);

        /// Opens a session with a shell of `kudari_core::shells`.
        #[qinvokable]
        fn add_session_with_shell(self: Pin<&mut Self>, shell: &QString);

        #[qinvokable]
        fn input_blocked(self: Pin<&mut Self>, terminal: i32);

        #[qinvokable]
        fn after_show(self: Pin<&mut Self>);

        /// Stores a setting from the settings dialog and applies it right away.
        #[qinvokable]
        fn set_setting(self: Pin<&mut Self>, key: &QString, value: &QString);

        /// Ends the access of every AI agent the user allowed.
        #[qinvokable]
        fn revoke_agent_access(self: Pin<&mut Self>);

        #[qinvokable]
        fn available_skins_json(self: &Self) -> QString;

        #[qinvokable]
        fn can_import_yakuake(self: &Self) -> bool;

        /// Gives an action other key sequences, separated by ";", for a front end without a
        /// shortcut dialog of its own.
        #[qinvokable]
        fn set_shortcut(self: Pin<&mut Self>, name: &QString, sequences: &QString);

        /// Makes `sequence` the global shortcut; false if the system refused it.
        #[qinvokable]
        fn set_toggle_shortcut(self: Pin<&mut Self>, sequence: &QString) -> bool;

        /// The key of a key event of QML with its modifiers, in the portable text of Qt.
        #[qinvokable]
        fn key_sequence_text(self: &Self, key: i32, modifiers: i32) -> QString;

        /// A sequence in portable text as the user's language names its keys.
        #[qinvokable]
        fn native_key_sequence(self: &Self, portable: &QString) -> QString;

        /// Gives a tab a color as `#rrggbb`, or takes it away with an empty one.
        #[qinvokable]
        fn choose_tab_color(self: Pin<&mut Self>, session: i32, color: &QString);

        /// The installed shells as `text` and `value` of the `Shell` setting.
        #[qinvokable]
        fn shells_json(self: &Self) -> QString;

        /// Whether the front end has a feature: `skins`, `remote-commands`, `key-tables`,
        /// `shortcut-dialog`, `shells` or `color-scheme`.
        #[qinvokable]
        fn supports(self: &Self, feature: &QString) -> bool;

        /// Takes over settings from Yakuake and Konsole. Returns false on failure.
        #[qinvokable]
        fn import_yakuake(self: Pin<&mut Self>) -> bool;

        /// Asks before importing into an existing setup, and imports if the user agrees.
        #[qinvokable]
        fn ask_import(self: Pin<&mut Self>);

        /// The global shortcut of this application, in portable text.
        #[qinvokable]
        fn toggle_shortcut(self: &Self) -> QString;

        /// The key Yakuake uses for its window, or an empty string.
        #[qinvokable]
        fn yakuake_shortcut(self: &Self) -> QString;

        /// Ends the first-run dialog; an empty `shortcut` keeps the current one.
        #[qinvokable]
        fn finish_first_run(self: Pin<&mut Self>, import: bool, shortcut: &QString);

        /// Highlights a terminal the user clicked into, if that is enabled.
        #[qinvokable]
        fn terminal_clicked(self: Pin<&mut Self>, terminal: i32);

        #[qinvokable]
        fn fixed_fonts_json(self: &Self) -> QString;

        /// The installed key binding files, as a JSON list of names.
        #[qinvokable]
        fn key_tables_json(self: &Self) -> QString;

        /// Offers actions for dropped files, one path per line.
        #[qinvokable]
        fn popup_drop_menu(self: &Self, x: i32, y: i32, terminal: i32, paths: &QString);

        /// The display text of an action, without accelerator marker.
        #[qinvokable]
        fn action_text(self: &Self, name: &QString) -> QString;

        #[qinvokable]
        fn output_names(self: &Self) -> QString;

        /// Translated UI text from the catalogue in `po/`.
        #[qinvokable]
        fn translate(self: &Self, context: &QString, text: &QString) -> QString;

        /// Like `translate`, for messages with a `%1` placeholder.
        #[qinvokable]
        fn translate_with(self: &Self, context: &QString, text: &QString, argument: &QString) -> QString;

        #[qsignal]
        fn terminal_dirty(self: Pin<&mut Self>, terminal: i32);

        #[qsignal]
        fn focus_terminal(self: Pin<&mut Self>, terminal: i32);

        #[qsignal]
        fn rename_requested(self: Pin<&mut Self>, session: i32);

        #[qsignal]
        fn highlight_terminal(self: Pin<&mut Self>, terminal: i32, persistent: bool);

        #[qsignal]
        fn remove_highlight(self: Pin<&mut Self>);

        #[qsignal]
        fn keyboard_blocked(self: Pin<&mut Self>, terminal: i32);

        /// A clipboard or selection action for the active terminal.
        #[qsignal]
        fn terminal_action(self: Pin<&mut Self>, terminal: i32, action: &QString);

        #[qsignal]
        fn quit_requested(self: Pin<&mut Self>);

        #[qsignal]
        fn first_run_requested(self: Pin<&mut Self>);

        /// Opens the settings; `module` selects a page and may be empty.
        #[qsignal]
        fn settings_requested(self: Pin<&mut Self>, module: &QString);

        /// A question for the QML of a front end without dialogs of its own; `confirm` is
        /// `quit`, `close` or `continue`. The answer goes to `answerQuestion`.
        #[qsignal]
        fn question_asked(self: Pin<&mut Self>, id: i32, title: &QString, text: &QString, confirm: &QString);

        #[qinvokable]
        fn answer_question(self: Pin<&mut Self>, id: i32, accepted: bool);

        /// The question whether an AI agent may use Kudari, for a front end without
        /// dialogs of its own. The answer goes to `answerAgent`.
        #[qsignal]
        fn agent_asked(
            self: Pin<&mut Self>,
            agent: &QString,
            title: &QString,
            text: &QString,
            allow: &QString,
            deny: &QString,
        );

        /// The agent of `agentAsked` went before the user answered.
        #[qsignal]
        fn agent_question_closed(self: Pin<&mut Self>);

        #[qinvokable]
        fn answer_agent(self: Pin<&mut Self>, agent: &QString, allowed: bool);
    }

    impl cxx_qt::Threading for Controller {}
    impl cxx_qt::Initialize for Controller {}
}

static THREAD: OnceLock<CxxQtThread<qobject::Controller>> = OnceLock::new();

thread_local! {
    static INSTANCE: Cell<*mut qobject::Controller> = const { Cell::new(std::ptr::null_mut()) };
}

/// Runs `action` on the Qt thread with the controller.
pub fn queue(action: impl FnOnce(Pin<&mut qobject::Controller>) + Send + 'static) {
    if let Some(thread) = THREAD.get() {
        let _ = thread.queue(action);
    }
}

/// Runs `action` right away. Must be called on the Qt thread.
pub fn with_controller(action: impl FnOnce(Pin<&mut qobject::Controller>)) {
    let pointer = INSTANCE.with(Cell::get);
    // SAFETY: the pointer is set once to the QML-owned controller, which lives until
    // the application exits, and it is only used on the Qt thread.
    if let Some(controller) = unsafe { pointer.as_mut() } {
        // SAFETY: Qt keeps a QObject in place, so the controller is never moved.
        action(unsafe { Pin::new_unchecked(controller) });
    }
}

pub struct ControllerRust {
    skin_json: QString,
    tabs_json: QString,
    layout_json: QString,
    terminal_ids_json: QString,
    settings_json: QString,
    shortcuts_json: QString,
    title: QString,
    fonts_json: QString,
    highlight_color: QString,
    output_name: QString,
    work_width: i32,
    work_height: i32,
    window_visible: bool,
    fullscreen: bool,
    dialog_open: bool,
    agent_access: bool,

    /// Sessions, monitors and the window, in the core.
    app: App,
    skin: theme::WindowColors,
}

impl Default for ControllerRust {
    fn default() -> Self {
        let settings = Settings::load(frontend().store());
        let skin = theme::window_colors(frontend(), &settings.skin).unwrap_or_default();
        Self {
            skin_json: QString::default(),
            tabs_json: QString::default(),
            layout_json: QString::default(),
            terminal_ids_json: QString::from("[]"),
            settings_json: QString::default(),
            shortcuts_json: QString::default(),
            title: QString::default(),
            fonts_json: QString::default(),
            highlight_color: QString::default(),
            output_name: QString::default(),
            work_width: 0,
            work_height: 0,
            window_visible: false,
            fullscreen: false,
            dialog_open: false,
            agent_access: false,
            app: App::new(settings),
            skin,
        }
    }
}

/// The terminals of this process, for the app.
struct Terminals;

impl Host for Terminals {
    fn working_directory(&self, terminal: TerminalId) -> Option<PathBuf> {
        terminal::backend(terminal)?.working_directory()
    }

    fn has_focus(&self) -> bool {
        qt::rk_application_has_focus()
    }

    fn terminal(&self, terminal: TerminalId) -> Option<std::sync::Arc<dyn kudari_core::agent::Terminal>> {
        terminal::backend(terminal).map(|backend| backend as std::sync::Arc<dyn kudari_core::agent::Terminal>)
    }
}

impl cxx_qt::Initialize for qobject::Controller {
    fn initialize(mut self: Pin<&mut Self>) {
        let _ = THREAD.set(self.qt_thread());
        terminal::listen();
        // SAFETY: only the address is stored, the object is not moved by QML.
        let pointer = unsafe { std::ptr::from_mut::<qobject::Controller>(self.as_mut().get_unchecked_mut()) };
        INSTANCE.with(|instance| instance.set(pointer));
        let skin = skin_to_json(&self.skin).to_string();
        self.as_mut().set_skin_json(qs(&skin));
        self.as_mut().apply_terminal_settings();
        self.as_mut().set_fonts_json(qt::rk_system_fonts_json());
        self.as_mut().set_highlight_color(qs(&frontend().highlight_color()));
        self.as_mut().set_shortcuts_json(qs(&frontend().shortcuts_json()));
        self.as_mut().publish_settings();
    }
}

fn skin_to_json(colors: &theme::WindowColors) -> Value {
    json!({"borderColor": colors.border})
}

fn color_name(color: alacritty_terminal::vte::ansi::Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn layout_to_json(layout: &Layout, active: TerminalId) -> Value {
    json!({
        "active": active,
        "terminals": layout.terminals.iter().map(|(id, r)| json!({
            "id": id, "x": r.x, "y": r.y, "width": r.width, "height": r.height
        })).collect::<Vec<_>>(),
        "handles": layout.handles.iter().enumerate().map(|(index, h)| json!({
            "index": index, "x": h.rect.x, "y": h.rect.y, "width": h.rect.width, "height": h.rect.height,
            "horizontal": h.orientation == Orientation::Horizontal,
        })).collect::<Vec<_>>(),
    })
}

/// What a file asked for with `rk_ask_save_file` is for.
const SAVE_OUTPUT: i32 = 0;
const AUTO_SAVE: i32 = 1;

impl qobject::Controller {
    /// Carries out what the app asks for, in order.
    fn apply(mut self: Pin<&mut Self>, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Publish => self.as_mut().sync(),
                Effect::PublishSettings => self.as_mut().publish_settings(),
                Effect::UpdateActions => self.as_mut().update_session_actions(),
                Effect::Stop(terminal) => terminal::stop(terminal),
                Effect::Repaint(terminal) => self.as_mut().terminal_dirty(terminal),
                Effect::Focus(terminal) => self.as_mut().focus_terminal(terminal),
                Effect::Highlight { terminal, persistent } => self.as_mut().highlight_terminal(terminal, persistent),
                Effect::RemoveHighlight => self.as_mut().remove_highlight(),
                Effect::Notify { event, text } => frontend().notify(event, &text),
                Effect::AskAgent(agent) => frontend().ask_agent(agent),
                Effect::CloseAgentQuestion => frontend().close_agent_question(),
                Effect::AgentAccess(active) => self.as_mut().set_agent_access(active),
                Effect::Ask(question) => {
                    if !frontend().ask(&question) {
                        let confirm = match question.confirm {
                            app::Confirm::Quit => "quit",
                            app::Confirm::Close => "close",
                            app::Confirm::Continue => "continue",
                        };
                        self.as_mut().question_asked(
                            question.id as i32,
                            &qs(&question.title),
                            &qs(&question.text),
                            &qs(confirm),
                        );
                    }
                }
                Effect::Quit => self.as_mut().quit_requested(),
                Effect::UpdateWorkArea => self.as_mut().update_work_area(),
                Effect::WindowVisible(visible) => self.as_mut().set_window_visible(visible),
                Effect::Refocus => {
                    self.as_mut().set_window_visible(false);
                    queue(|controller| controller.set_window_visible(true));
                }
                Effect::Fullscreen(fullscreen) => {
                    self.as_mut().set_fullscreen(fullscreen);
                    frontend().set_action_state("view-full-screen", fullscreen, true);
                }
                Effect::RestartSilence(terminal) => {
                    if let Some(backend) = terminal::backend(terminal) {
                        *backend
                            .last_output
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
                    }
                }
                Effect::AutoSave => {
                    for terminal in self.app.stack().all_terminals() {
                        terminal::auto_save(terminal);
                    }
                }
                Effect::RenameTab(session) => self.as_mut().rename_requested(session),
                Effect::FirstRun => self.as_mut().first_run_requested(),
                Effect::OpenSettings(module) => self.as_mut().settings_requested(&qs(module.unwrap_or_default())),
                Effect::TerminalAction { terminal, action } => self.as_mut().terminal_action(terminal, &qs(&action)),
                Effect::Terminal { terminal, command } => self.as_mut().run_terminal_command(terminal, command),
                Effect::Import => {
                    self.as_mut().import_yakuake();
                }
            }
        }
    }

    /// Runs `action` on the app and carries out its effects.
    fn update(mut self: Pin<&mut Self>, action: impl FnOnce(&mut App) -> Vec<Effect>) {
        let effects = action(&mut self.as_mut().rust_mut().app);
        self.apply(effects);
    }

    fn publish_settings(self: Pin<&mut Self>) {
        let s = self.app.settings();
        let value = json!({
            "width": s.width, "height": s.height, "position": s.position, "screen": s.screen,
            "keepOpen": s.keep_open, "showTabBar": s.show_tab_bar,
            "translucency": s.translucency, "backgroundColor": s.background_color,
            "backgroundOpacity": s.background_color_opacity, "hideSkinBorders": s.hide_skin_borders,
            "focusFollowsMouse": s.focus_follows_mouse,
            "highlightOpacity": s.terminal_highlight_opacity,
            "highlightDuration": s.terminal_highlight_duration,
            "highlightOnManualActivation": s.terminal_highlight_on_manual_activation,
            "blockColor": s.keyboard_block_color, "blockOpacity": s.keyboard_block_opacity,
            "blockDuration": s.keyboard_block_duration,
            "config": s.to_config_json(),
            "terminalBackground": color_name(colors::active().background),
            "terminalForeground": color_name(colors::active().foreground),
        })
        .to_string();
        let menu = json!({
            "width": s.width, "height": s.height, "screen": s.screen,
            "screens": frontend().outputs().len(),
        })
        .to_string();
        let keep_open = s.keep_open;
        frontend().set_menu_state(&menu);
        frontend().set_action_state("keep-open", keep_open, true);
        self.set_settings_json(qs(&value));
    }

    /// Starts shells for new terminals and publishes the model to QML.
    fn sync(mut self: Pin<&mut Self>) {
        let (cell_width, cell_height) = self.app.cell();
        let mut started = Vec::new();
        for start in self.app.pending_starts() {
            let (columns, lines) = terminal::grid_size(start.rect.width, start.rect.height, cell_width, cell_height);
            let size = (cell_width as u16, cell_height as u16);
            let shell = start.shell.as_deref();
            match terminal::start(start.terminal, columns, lines, size.0, size.1, start.directory, shell) {
                Ok(()) => started.push(start.terminal),
                Err(error) => eprintln!("kudari: could not start a shell: {error}"),
            }
        }
        self.as_mut().rust_mut().app.started(&started);

        let model = self.as_mut().rust_mut().app.model();
        let tabs: Vec<Value> = model
            .tabs
            .iter()
            .map(|tab| {
                json!({
                    "id": tab.id,
                    "title": tab.title,
                    "selected": tab.selected,
                    "closable": tab.closable,
                    "color": tab.color,
                    "activity": tab.activity,
                })
            })
            .collect();
        let mut layout = model.layout.as_ref().map_or_else(
            || json!({"active": -1, "terminals": [], "handles": []}),
            |(layout, active)| layout_to_json(layout, *active),
        );
        let font_zoom: serde_json::Map<String, Value> = model
            .font_zoom
            .iter()
            .map(|(id, zoom)| (id.to_string(), Value::from(*zoom)))
            .collect();
        if let Some(fields) = layout.as_object_mut() {
            fields.insert("keyboardDisabled".to_string(), Value::from(model.keyboard_disabled));
            fields.insert("fontZoom".to_string(), Value::Object(font_zoom));
        }

        self.as_mut().update_session_actions();
        self.as_mut()
            .set_terminal_ids_json(qs(&Value::from(model.terminals).to_string()));
        self.as_mut().set_tabs_json(qs(&Value::from(tabs).to_string()));
        self.as_mut().set_layout_json(qs(&layout.to_string()));
        self.as_mut().set_title(qs(&model.title));
    }

    /// Mirrors `TabBar::updateMoveActions` and `updateToggleActions`.
    fn update_session_actions(self: Pin<&mut Self>) {
        for (name, checked, enabled) in self.app.session_actions() {
            frontend().set_action_state(name, checked, enabled);
        }
    }

    fn start(self: Pin<&mut Self>) {
        self.update(App::start);
    }

    fn set_geometry(self: Pin<&mut Self>, width: i32, height: i32, cell_width: f64, cell_height: f64) {
        self.update(|app| app.set_geometry(width, height, cell_width, cell_height));
    }

    /// Opens a session whose terminals start in the directory of the active terminal.
    pub fn add_session(mut self: Pin<&mut Self>, kind: SessionType) -> SessionId {
        let (id, effects) = self.as_mut().rust_mut().app.add_session(kind, &Terminals);
        self.apply(effects);
        id
    }

    fn add_session_action(self: Pin<&mut Self>) {
        self.add_session(SessionType::Single);
    }

    /// Closes a session, after asking if it is protected.
    pub fn remove_session(self: Pin<&mut Self>, session: SessionId) {
        self.update(|app| app.remove_session(session));
    }

    /// Removes a terminal from its session, closing the session when it was the last one.
    pub fn close_terminal(self: Pin<&mut Self>, terminal: TerminalId) {
        self.update(|app| app.close_terminal(terminal));
    }

    pub fn terminal_exited(self: Pin<&mut Self>, terminal: TerminalId) {
        self.update(|app| app.terminal_exited(terminal));
    }

    pub fn terminal_output(self: Pin<&mut Self>, terminal: TerminalId) {
        if let Some(backend) = terminal::backend(terminal) {
            *backend
                .last_output
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
        }
        self.update(|app| app.terminal_output(terminal));
    }

    pub fn terminal_bell(self: Pin<&mut Self>, terminal: TerminalId) {
        self.update(|app| app.terminal_bell(terminal));
    }

    fn answer_question(self: Pin<&mut Self>, id: i32, accepted: bool) {
        self.answer(id, accepted);
    }

    /// The answer of the user to a question of the app.
    pub fn answer(self: Pin<&mut Self>, id: i32, accepted: bool) {
        if let Ok(id) = u32::try_from(id) {
            self.update(|app| app.answer(id, accepted));
        }
    }

    /// Periodic update of titles and the monitors.
    fn poll(self: Pin<&mut Self>) {
        let format = self.app.settings().title_format.clone();
        let statuses: Vec<app::Status> = self
            .app
            .stack()
            .all_terminals()
            .into_iter()
            .filter_map(|terminal| {
                let backend = terminal::backend(terminal)?;
                let quiet_for = backend
                    .last_output
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .elapsed();
                Some(app::Status {
                    terminal,
                    title: backend.title(&format),
                    busy: backend.is_busy(),
                    quiet_for,
                })
            })
            .collect();
        self.update(|app| app.poll(&statuses));
    }

    pub fn split(mut self: Pin<&mut Self>, session: SessionId, orientation: Option<Orientation>) -> i32 {
        let (new, effects) = self.as_mut().rust_mut().app.split(session, orientation, &Terminals);
        self.apply(effects);
        new.unwrap_or(-1)
    }

    pub fn raise(self: Pin<&mut Self>, session: SessionId) {
        self.update(|app| app.raise(session));
    }

    fn select_tab(self: Pin<&mut Self>, session: i32) {
        self.raise(session);
    }

    fn close_tab(self: Pin<&mut Self>, session: i32) {
        self.remove_session(session);
    }

    fn close_active_tab(self: Pin<&mut Self>) {
        if let Some(session) = self.app.stack().active_id() {
            self.remove_session(session);
        }
    }

    fn select_next_tab(self: Pin<&mut Self>) {
        self.update(App::select_next_tab);
    }

    fn select_previous_tab(self: Pin<&mut Self>) {
        self.update(App::select_previous_tab);
    }

    fn drop_tab(self: Pin<&mut Self>, session: i32, target: i32) {
        self.update(|app| app.drop_tab(session, usize::try_from(target).ok()));
    }

    fn rename_tab(self: Pin<&mut Self>, session: i32, title: &QString) {
        let title = title.to_string();
        self.update(|app| app.rename_tab(session, &title));
    }

    /// Highlights a terminal the user clicked into, if that is enabled and there are several.
    fn terminal_clicked(self: Pin<&mut Self>, terminal: i32) {
        self.update(|app| app.terminal_clicked(terminal));
    }

    fn zoom_terminal(self: Pin<&mut Self>, terminal: i32, delta: i32) {
        self.update(|app| app.zoom_terminal(terminal, delta));
    }

    fn activate_terminal(self: Pin<&mut Self>, terminal: i32) {
        self.update(|app| app.activate_terminal(terminal));
    }

    fn drag_handle(self: Pin<&mut Self>, handle: i32, delta: i32) {
        if let Ok(handle) = usize::try_from(handle) {
            self.update(|app| app.drag_handle(handle, delta));
        }
    }

    fn set_keep_open(self: Pin<&mut Self>, keep_open: bool) {
        self.update(|app| app.set_keep_open(frontend().store(), keep_open));
    }

    fn set_height_percent(self: Pin<&mut Self>, height: i32) {
        self.update(|app| app.set_window_size(frontend().store(), "height", height));
    }

    fn application_has_focus(&self) -> bool {
        qt::rk_application_has_focus()
    }

    /// `MainWindow::wmActiveWindowChanged`: own menus and dialogs do not retract.
    fn focus_lost(self: Pin<&mut Self>) {
        let has_focus = qt::rk_application_has_focus();
        let dialog_open = *self.dialog_open();
        self.update(|app| app.focus_lost(has_focus, dialog_open));
    }

    /// Translates a text of the QML pages, which the check of the translations finds there.
    fn translate(&self, context: &QString, text: &QString) -> QString {
        let context = &context.to_string();
        qs(&tr(context, &text.to_string()))
    }

    fn set_setting(self: Pin<&mut Self>, key: &QString, value: &QString) {
        if settings::store(frontend().store(), &key.to_string(), &value.to_string()) {
            self.reload_settings();
        }
    }

    fn can_import_yakuake(&self) -> bool {
        frontend().can_import()
    }

    fn set_shortcut(self: Pin<&mut Self>, name: &QString, sequences: &QString) {
        let sequences: Vec<String> = sequences
            .to_string()
            .split(';')
            .map(str::trim)
            .filter(|sequence| !sequence.is_empty())
            .map(str::to_string)
            .collect();
        frontend().set_shortcut(&name.to_string(), &sequences);
        self.shortcuts_changed();
    }

    fn set_toggle_shortcut(self: Pin<&mut Self>, sequence: &QString) -> bool {
        frontend().set_toggle(&sequence.to_string())
    }

    fn key_sequence_text(&self, key: i32, modifiers: i32) -> QString {
        qt::rk_key_sequence_text(key, modifiers)
    }

    fn native_key_sequence(&self, portable: &QString) -> QString {
        qt::rk_native_key_sequence(portable)
    }

    fn choose_tab_color(self: Pin<&mut Self>, session: i32, color: &QString) {
        let color = color.to_string();
        let color =
            kudari_core::colors::parse_color(&color).map(|rgb| format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b));
        self.update(|app| app.set_tab_color(session, color));
    }

    fn shells_json(&self) -> QString {
        qs(&kudari_core::menus::shell_choices(kudari_core::shells::installed()).to_string())
    }

    fn supports(&self, feature: &QString) -> bool {
        crate::frontend::Feature::from_name(&feature.to_string()).is_some_and(|feature| frontend().supports(feature))
    }

    fn import_yakuake(mut self: Pin<&mut Self>) -> bool {
        if !frontend().import() {
            return false;
        }
        frontend().reload_shortcuts();
        self.as_mut().shortcuts_changed();
        self.reload_settings();
        true
    }

    fn ask_import(self: Pin<&mut Self>) {
        self.update(App::ask_import);
    }

    fn toggle_shortcut(&self) -> QString {
        qs(&frontend().toggle())
    }

    fn yakuake_shortcut(&self) -> QString {
        qs(&frontend().foreign_toggle())
    }

    fn finish_first_run(mut self: Pin<&mut Self>, import: bool, shortcut: &QString) {
        settings::mark_first_run_done(frontend().store());
        if !shortcut.is_empty() {
            frontend().set_toggle(&shortcut.to_string());
        }
        if import {
            self.as_mut().import_yakuake();
        }
        self.reload_settings();
    }

    fn reload_settings(mut self: Pin<&mut Self>) {
        let settings = Settings::load(frontend().store());
        frontend().set_tray_visible(settings.show_systray_icon);
        let old = self.as_mut().rust_mut().app.replace_settings(settings);
        let new = self.app.settings();
        let skin_changed = new.skin != old.skin;
        // Agents were allowed for what the settings permitted then; any change asks again.
        let agents_changed =
            (new.allow_agent_reading, new.allow_agent_control) != (old.allow_agent_reading, old.allow_agent_control);
        self.as_mut().apply_terminal_settings();
        if skin_changed {
            self.as_mut().apply_skin();
        }
        self.as_mut().update_work_area();
        self.as_mut().publish_settings();
        self.as_mut().sync();
        if agents_changed {
            self.as_mut().update(App::forget_agents);
        }
        for terminal in self.app.stack().all_terminals() {
            self.as_mut().terminal_dirty(terminal);
        }
    }

    fn apply_skin(mut self: Pin<&mut Self>) {
        let Some(skin) = theme::window_colors(frontend(), &self.app.settings().skin) else {
            return;
        };
        let json = skin_to_json(&skin).to_string();
        self.as_mut().rust_mut().skin = skin;
        self.as_mut().set_skin_json(qs(&json));
    }

    /// The user switched the Plasma theme or its colors.
    pub fn theme_changed(mut self: Pin<&mut Self>) {
        self.as_mut().set_highlight_color(qs(&frontend().highlight_color()));
        self.as_mut().apply_skin();
        self.as_mut().apply_terminal_settings();
        self.publish_settings();
    }

    /// Colors, history and cursor apply to running terminals as well.
    fn apply_terminal_settings(mut self: Pin<&mut Self>) {
        let settings = self.app.settings();
        // A change of the scheme changes the palette, and the terminal colors follow it then.
        frontend().set_color_scheme(&settings.color_scheme);
        if let Some(scheme) = theme::terminal_scheme(frontend(), &settings.skin) {
            colors::set_scheme(scheme);
        }
        terminal::apply_options(terminal::Options::from_settings(settings));
        terminal::set_scrollbar_visible(settings.scroll_bar_position != "hidden");
        frontend().set_blur(settings.blurs_behind());
        // An idle terminal would otherwise show the new options only after its next output.
        for terminal in self.app.stack().all_terminals() {
            self.as_mut().terminal_dirty(terminal);
        }
    }

    fn fixed_fonts_json(&self) -> QString {
        qt::rk_fixed_fonts()
    }

    fn action_text(&self, name: &QString) -> QString {
        qs(&frontend().action_text(&name.to_string()))
    }

    fn available_skins_json(&self) -> QString {
        let list: Vec<Value> = frontend()
            .skins()
            .into_iter()
            .map(|skin| json!({"id": skin.id, "name": skin.name, "author": skin.description}))
            .collect();
        qs(&Value::from(list).to_string())
    }

    fn key_tables_json(&self) -> QString {
        qs(&Value::from(crate::keytab::available()).to_string())
    }

    fn popup_drop_menu(&self, x: i32, y: i32, terminal: i32, paths: &QString) {
        let directory = terminal::backend(terminal)
            .and_then(|backend| backend.working_directory())
            .map(|directory| directory.to_string_lossy().into_owned())
            .unwrap_or_default();
        frontend().popup_drop_menu(x, y, terminal, &paths.to_string(), &directory);
    }

    fn output_names(&self) -> QString {
        qs(&frontend().outputs().join(","))
    }

    pub fn allows_remote_commands(&self) -> bool {
        self.app.settings().allow_remote_commands
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    /// A request of the AI agent with the connection `agent`.
    ///
    /// # Errors
    /// Why the request was refused, with a sentence for the agent.
    pub fn agent_request(
        mut self: Pin<&mut Self>,
        agent: Option<&str>,
        request: kudari_core::agent::Request,
    ) -> Result<String, kudari_core::agent::Error> {
        let (result, effects) = self.as_mut().rust_mut().app.agent_request(agent, request, &Terminals);
        self.apply(effects);
        result
    }

    /// The user answered the question about an agent.
    pub fn agent_answered(self: Pin<&mut Self>, agent: &str, allowed: bool) {
        self.update(|app| app.agent_answered(agent, allowed));
    }

    /// A connection to the bus ended; if it was an agent, its access ends with it.
    pub fn agent_gone(self: Pin<&mut Self>, name: &str) {
        self.update(|app| app.agent_gone(name));
    }

    /// Asks the user about `agent` through `agentAsked`, with the name of its program,
    /// unless the question is no longer open. Returns whether it asked.
    pub fn ask_about_agent(self: Pin<&mut Self>, agent: &str, program: Option<&str>) -> bool {
        let Some(text) = self.app.agent_question(agent, program) else {
            return false;
        };
        self.agent_asked(
            &qs(agent),
            &qs(&tr("@title:window", "AI Agent")),
            &qs(&text),
            &qs(&tr("@action:button", "Allow")),
            &qs(&tr("@action:button", "Deny")),
        );
        true
    }

    fn answer_agent(self: Pin<&mut Self>, agent: &QString, allowed: bool) {
        self.agent_answered(&agent.to_string(), allowed);
    }

    /// The user ended the access of the agents from the bar.
    pub fn revoke_agent_access(self: Pin<&mut Self>) {
        self.update(App::revoke_agents);
    }

    fn translate_with(&self, context: &QString, text: &QString, argument: &QString) -> QString {
        let context = &context.to_string();
        qs(&kudari_core::i18n::tr_arg(
            context,
            &text.to_string(),
            &argument.to_string(),
        ))
    }

    fn after_show(mut self: Pin<&mut Self>) {
        let (slide, effects) = self.as_mut().rust_mut().app.after_show();
        if slide {
            frontend().slide();
        }
        frontend().set_blur(self.app.settings().blurs_behind());
        self.apply(effects);
    }

    /// The screen to open on and the area that panels leave free on it.
    fn update_work_area(mut self: Pin<&mut Self>) {
        let area = services::work_area(frontend(), self.app.settings().screen);
        self.as_mut().set_output_name(qs(&area.output));
        self.as_mut().set_work_width(area.width);
        self.as_mut().set_work_height(area.height);
    }

    /// `MainWindow::_toggleWindowState` for Wayland, where the compositor animates.
    pub fn toggle_window(self: Pin<&mut Self>) {
        let has_focus = Terminals.has_focus();
        self.update(|app| app.toggle_window(has_focus));
    }

    fn tab_menu_state(&self, session: SessionId) -> String {
        let found = self.app.stack().session(session);
        let terminals: Vec<Value> = found
            .map(|found| {
                found
                    .terminals
                    .iter()
                    .map(|(id, state)| {
                        json!({"id": id, "keyboardDisabled": !state.keyboard_input_enabled,
                               "activity": state.monitor_activity, "silence": state.monitor_silence})
                    })
                    .collect()
            })
            .unwrap_or_default();
        let color = found.and_then(|found| found.color.clone()).unwrap_or_default();
        json!({ "terminals": terminals, "session": session, "color": color }).to_string()
    }

    fn show_menu(self: Pin<&mut Self>, x: i32, y: i32, menu: Option<String>) {
        if let Some(menu) = menu {
            self.menu_requested(x, y, &qs(&menu));
        }
    }

    fn popup_main_menu(self: Pin<&mut Self>, x: i32, y: i32) {
        let menu = frontend().popup_main_menu(x, y);
        self.show_menu(x, y, menu);
    }

    fn popup_session_menu(self: Pin<&mut Self>, x: i32, y: i32) {
        let menu = frontend().popup_session_menu(x, y);
        self.show_menu(x, y, menu);
    }

    fn popup_tab_menu(mut self: Pin<&mut Self>, session: i32, x: i32, y: i32) {
        self.as_mut().update(|app| app.menu_opened(session));
        let state = self.tab_menu_state(session);
        let menu = frontend().popup_tab_menu(x, y, &state);
        self.show_menu(x, y, menu);
    }

    fn toggle(self: Pin<&mut Self>, name: &QString, checked: bool) {
        self.trigger_action(&name.to_string(), checked);
    }

    fn choose(self: Pin<&mut Self>, kind: &QString, value: i32) {
        self.menu_value(&kind.to_string(), value);
    }

    fn use_link(self: Pin<&mut Self>, link: &QString, copy: bool) {
        let link = link.to_string();
        if copy {
            frontend().copy(&link, crate::services::Selection::Clipboard);
        } else {
            terminal::open_link(&link);
        }
    }

    fn add_session_with_shell(mut self: Pin<&mut Self>, shell: &QString) {
        let shell = shell.to_string();
        let (_, effects) =
            self.as_mut()
                .rust_mut()
                .app
                .add_session_with_shell(SessionType::Single, Some(&shell), &Terminals);
        self.apply(effects);
    }

    fn popup_terminal_menu(mut self: Pin<&mut Self>, x: i32, y: i32, can_copy: bool, link: &QString) {
        let backend = self.app.active_terminal().and_then(terminal::backend);
        let auto_saving = backend
            .as_ref()
            .is_some_and(|backend| backend.auto_save.lock().is_ok_and(|path| path.is_some()));
        let tracks_mouse = backend.is_some_and(|backend| backend.tracks_mouse());
        frontend().set_action_state("allow-mouse-tracking", tracks_mouse, true);
        let menu = frontend().popup_terminal_menu(x, y, can_copy, &link.to_string(), auto_saving);
        self.as_mut().show_menu(x, y, menu);
    }

    fn input_blocked(self: Pin<&mut Self>, terminal: i32) {
        self.keyboard_blocked(terminal);
    }

    /// Sets the color of a tab; a negative value goes back to the configured one.
    pub fn set_tab_color(self: Pin<&mut Self>, session: SessionId, color: i32) {
        let color = (color >= 0).then(|| format!("#{color:06x}"));
        self.update(|app| app.set_tab_color(session, color));
    }

    pub fn menu_value(self: Pin<&mut Self>, kind: &str, value: i32) {
        if kind == "signal" {
            if let Some(backend) = self.app.active_terminal().and_then(terminal::backend)
                && let Some(signal) = crate::process::Signal::from_number(value)
            {
                backend.send_signal(signal);
            }
            return;
        }
        self.update(|app| app.set_window_size(frontend().store(), kind, value));
    }

    pub fn terminal_toggled(self: Pin<&mut Self>, kind: &str, terminal: i32, checked: bool) {
        if let Some(monitor) = Monitor::named(kind) {
            self.update(|app| app.terminal_toggled(monitor, terminal, checked));
        }
    }

    pub fn menu_hovered(mut self: Pin<&mut Self>, terminal: i32) {
        if terminal >= 0 {
            self.as_mut().highlight_terminal(terminal, true);
        } else {
            self.as_mut().remove_highlight();
        }
    }

    pub fn menu_closed(mut self: Pin<&mut Self>) {
        self.as_mut().remove_highlight();
        // The triggered action is queued behind this call and still needs the session.
        queue(|controller| controller.update(App::menu_closed));
    }

    pub fn dialog_active(mut self: Pin<&mut Self>, active: bool) {
        self.as_mut().set_dialog_open(active);
        if !active {
            self.as_mut().rust_mut().app.dialog_closed();
        }
    }

    pub fn shortcuts_changed(self: Pin<&mut Self>) {
        self.set_shortcuts_json(qs(&frontend().shortcuts_json()));
    }

    pub fn set_all_monitor(self: Pin<&mut Self>, session: SessionId, kind: &str, checked: bool) {
        if let Some(monitor) = Monitor::named(kind) {
            self.update(|app| app.set_all_monitor(session, monitor, checked));
        }
    }

    fn trigger(self: Pin<&mut Self>, name: &QString) {
        self.trigger_action(&name.to_string(), false);
    }

    pub fn trigger_action(mut self: Pin<&mut Self>, name: &str, checked: bool) {
        let effects = self
            .as_mut()
            .rust_mut()
            .app
            .trigger(name, checked, frontend().store(), &Terminals);
        self.apply(effects);
    }

    /// What only the program of a terminal can do, for an action.
    fn run_terminal_command(mut self: Pin<&mut Self>, terminal: TerminalId, command: TerminalCommand) {
        let Some(backend) = terminal::backend(terminal) else {
            return;
        };
        match command {
            TerminalCommand::AdjustHistory => qt::rk_ask_history_lines(
                terminal,
                backend.history_lines() as i32,
                &qs(&tr("@title:window", "Adjust Scrollback")),
                &qs(&tr("@label:spinbox", "Lines kept in this terminal:")),
            ),
            TerminalCommand::MouseTracking(enabled) => backend.set_tracks_mouse(enabled),
            TerminalCommand::OpenFolder => {
                if let Some(directory) = self.app.active_directory(&Terminals) {
                    frontend().open_url(&directory.to_string_lossy());
                }
            }
            TerminalCommand::WebSearch => {
                let text = backend.term.lock().selection_to_string();
                if let Some(text) = text {
                    frontend().web_search(&text);
                }
            }
            TerminalCommand::Reset => {
                backend.reset();
                self.as_mut().terminal_dirty(terminal);
            }
            TerminalCommand::SaveOutput => qt::rk_ask_save_file(terminal, SAVE_OUTPUT, &qs("output.txt")),
            TerminalCommand::Print => qt::rk_print_text(&qs(&backend.full_text())),
            TerminalCommand::AutoSave(true) => qt::rk_ask_save_file(terminal, AUTO_SAVE, &qs("output.txt")),
            TerminalCommand::AutoSave(false) => {
                *backend
                    .auto_save
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            TerminalCommand::Paste(text) => {
                let prepared = crate::input::prepare_paste(&text, backend.bracketed_paste());
                backend.write_user_input(prepared.into_bytes());
            }
        }
    }

    /// The user chose the scrollback size of a terminal.
    pub fn history_lines_chosen(mut self: Pin<&mut Self>, terminal: TerminalId, lines: i32) {
        if let Some(backend) = terminal::backend(terminal) {
            backend.set_history_lines(lines.max(0) as usize);
            self.as_mut().terminal_dirty(terminal);
        }
    }

    /// The user chose a file for the output of a terminal: to save it now with
    /// [`SAVE_OUTPUT`], or from now on with [`AUTO_SAVE`].
    pub fn file_chosen(&self, terminal: TerminalId, purpose: i32, path: PathBuf) {
        let Some(backend) = terminal::backend(terminal) else {
            return;
        };
        if purpose == AUTO_SAVE {
            *backend
                .auto_save
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
        } else if let Err(error) = backend.save_output(&path) {
            eprintln!("kudari: could not save the output: {error}");
        }
    }

    /// Asks before pasting text whose lines would run right away.
    pub fn ask_paste(self: Pin<&mut Self>, terminal: TerminalId, text: String) {
        self.update(|app| app.ask_paste(terminal, text));
    }

    #[cfg(feature = "test-hooks")]
    pub fn set_setting_from_test(self: Pin<&mut Self>, key: &str, value: &str) {
        self.set_setting(&qs(key), &qs(value));
    }

    #[cfg(feature = "test-hooks")]
    pub fn popup_drop_active(&self, paths: &str) {
        if let Some(terminal) = self.app.active_terminal() {
            self.popup_drop_menu(300, 100, terminal, &qs(paths));
        }
    }

    #[cfg(feature = "test-hooks")]
    pub fn test_popup_tab_menu(self: Pin<&mut Self>) {
        if let Some(session) = self.app.stack().active_id() {
            self.popup_tab_menu(session, 120, 370);
        }
    }

    /// Read access for the D-Bus interface.
    pub fn stack(&self) -> &SessionStack {
        self.app.stack()
    }

    pub fn stack_mut(self: Pin<&mut Self>) -> Pin<&mut SessionStack> {
        let rust = self.rust_mut();
        // SAFETY: `SessionStack` is `Unpin`.
        unsafe { rust.map_unchecked_mut(|rust| rust.app.stack_mut()) }
    }

    pub fn area(&self) -> (i32, i32) {
        self.app.area()
    }

    pub fn refresh(self: Pin<&mut Self>) {
        self.update(|app| app.refresh());
    }

    pub fn run_command(&self, terminal: TerminalId, command: &str) {
        if let Some(backend) = terminal::backend(terminal) {
            // Scripts written for Yakuake expect the command to end with Return.
            backend.write(format!("{command}\r").into_bytes());
        }
    }
}
