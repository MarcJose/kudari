// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Access to the C++ layer in `cpp/kdebridge.cpp`.

use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;
    }

    unsafe extern "C++" {
        include!("kdebridge.h");
        /// Provides `i18n` functions for KDE components written in QML.
        fn rk_setup_qml_i18n(engine: Pin<&mut QQmlApplicationEngine>);
        /// The colors of a global theme, or of the active one for an empty id.
        fn rk_theme_colors(theme: &QString) -> QString;
        /// The installed global themes as a JSON list of `id` and `name`.
        fn rk_global_themes_json() -> QString;
        /// Offers what to do with dropped files; `paths` has one file per line.
        fn rk_popup_drop_menu(x: i32, y: i32, terminal: i32, paths: &QString, directory: &QString);

        fn rk_command_line(version: &QString);
        fn rk_init();
        fn rk_finish_startup(show_tray: bool);
        fn rk_config_read(group: &QString, key: &QString, fallback: &QString) -> QString;
        fn rk_config_write(group: &QString, key: &QString, value: &QString);
        fn rk_shortcuts_json() -> QString;
        fn rk_highlight_color() -> QString;
        fn rk_available_screen_rect(output: &QString) -> QString;
        fn rk_active_output_name() -> QString;
        fn rk_output_names() -> QString;
        fn rk_set_action_state(name: &QString, checked: bool, enabled: bool);
        fn rk_set_menu_state(json: &QString);
        fn rk_set_tray_visible(visible: bool);
        fn rk_reload_shortcuts();
        fn rk_toggle_shortcut() -> QString;
        fn rk_foreign_toggle_shortcut() -> QString;
        fn rk_set_toggle_shortcut(sequence: &QString);
        fn rk_popup_main_menu(x: i32, y: i32);
        fn rk_popup_session_menu(x: i32, y: i32);
        fn rk_popup_tab_menu(x: i32, y: i32, json: &QString);
        fn rk_popup_terminal_menu(x: i32, y: i32, can_copy: bool, link: &QString, auto_saving: bool);
        fn rk_notify(event: &QString, text: &QString);
        fn rk_slide_window();
        /// Asks without blocking whether an AI agent may use Kudari; the answer arrives in
        /// `kde_agent_answered`.
        fn rk_ask_agent(agent: &QString, title: &QString, text: &QString, allow: &QString, deny: &QString);
        /// Closes the question of `rk_ask_agent` without an answer.
        fn rk_close_agent_question();
        /// Asks without blocking; the answer arrives in `kde_question_answered`.
        fn rk_ask_question(id: i32, text: &QString, title: &QString, continue_kind: i32);
        fn rk_tr(context: &QString, text: &QString, argument: &QString) -> QString;
        fn rk_action_text(name: &QString) -> QString;
        fn rk_open_url(url: &QString);
        fn rk_web_search(text: &QString);
        fn rk_set_blur(enabled: bool);
        fn rk_test_key(sequence: &QString);
        fn rk_test_input_method(preedit: &QString, commit: &QString);
        fn rk_test_window_state() -> QString;
        fn rk_test_grab(path: &QString);
        fn rk_test_agent_question() -> QString;
        fn rk_test_answer_agent(allow: bool);
    }

    extern "Rust" {
        fn kde_action_triggered(name: &QString, checked: bool);
        fn kde_menu_value(kind: &QString, value: i32);
        fn kde_tab_color(session: i32, color: i32);
        fn kde_open_link(link: &QString);
        fn kde_theme_changed();
        fn kde_terminal_paths(terminal: i32, paths: &QString, change_directory: bool);
        fn kde_terminal_toggled(kind: &QString, terminal: i32, checked: bool);
        fn kde_menu_hovered(terminal: i32);
        fn kde_menu_closed();
        fn kde_dialog_active(active: bool);
        fn kde_shortcuts_changed();
        fn kde_agent_answered(agent: &QString, allowed: bool);
        fn kde_question_answered(id: i32, accepted: bool);
    }
}

pub use ffi::*;

/// Shorthand for building a `QString`.
pub fn qs(text: &str) -> QString {
    QString::from(text)
}

/// Button label of a confirmation dialog.
#[derive(Clone, Copy)]
#[repr(i32)]
pub enum Confirm {
    Quit = 0,
    Close = 1,
    Continue = 2,
}

/// Translates through the catalogue in `po/`.
pub fn tr(context: &str, text: &str) -> String {
    tr_arg(context, text, "")
}

/// Translates a message with a `%1` placeholder.
pub fn tr_arg(context: &str, text: &str, argument: &str) -> String {
    rk_tr(&qs(context), &qs(text), &qs(argument)).to_string()
}

fn kde_tab_color(session: i32, color: i32) {
    crate::controller::queue(move |controller| controller.set_tab_color(session, color));
}

fn kde_terminal_paths(terminal: i32, paths: &QString, change_directory: bool) {
    let paths = paths.to_string();
    crate::controller::queue(move |_| crate::terminal::send_paths(terminal, &paths, change_directory));
}

fn kde_theme_changed() {
    crate::controller::queue(super::controller::qobject::Controller::theme_changed);
}

fn kde_open_link(link: &QString) {
    crate::terminal::open_link(&link.to_string());
}

fn kde_action_triggered(name: &QString, checked: bool) {
    let name = name.to_string();
    crate::controller::queue(move |controller| controller.trigger_action(&name, checked));
}

fn kde_menu_value(kind: &QString, value: i32) {
    let kind = kind.to_string();
    crate::controller::queue(move |controller| controller.menu_value(&kind, value));
}

fn kde_terminal_toggled(kind: &QString, terminal: i32, checked: bool) {
    let kind = kind.to_string();
    crate::controller::queue(move |controller| controller.terminal_toggled(&kind, terminal, checked));
}

fn kde_menu_hovered(terminal: i32) {
    crate::controller::queue(move |controller| controller.menu_hovered(terminal));
}

fn kde_menu_closed() {
    crate::controller::queue(super::controller::qobject::Controller::menu_closed);
}

/// Runs directly: the dialog blocks the Qt thread right after this call.
fn kde_dialog_active(active: bool) {
    crate::controller::with_controller(|controller| controller.dialog_active(active));
}

fn kde_shortcuts_changed() {
    crate::controller::queue(super::controller::qobject::Controller::shortcuts_changed);
}

/// Queued: the dialog can close while the controller is in use, for example when the agent
/// it asks about disconnects.
fn kde_agent_answered(agent: &QString, allowed: bool) {
    let agent = agent.to_string();
    crate::controller::queue(move |controller| controller.agent_answered(&agent, allowed));
}

/// Queued: the dialog closes from within the event loop of Qt.
fn kde_question_answered(id: i32, accepted: bool) {
    crate::controller::queue(move |controller| controller.answer(id, accepted));
}
