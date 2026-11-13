// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Access to the C++ layer in `cpp/qtbridge.cpp`: what the front ends need of Qt itself.

#![allow(
    clippy::must_use_candidate,
    reason = "cxx-qt generates public functions without these attributes"
)]

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qpainter.h");
        type QPainter = cxx_qt_lib::QPainter;
        include!("cxx-qt-lib/qfont.h");
        type QFont = cxx_qt_lib::QFont;
    }

    unsafe extern "C++" {
        include!("qtbridge.h");
        /// Draws the badge into the top right corner of a terminal.
        #[allow(clippy::too_many_arguments)]
        fn rk_draw_badge(
            painter: Pin<&mut QPainter>,
            width: f64,
            text: &QString,
            color: &QString,
            family: &QString,
            size: i32,
            text_only: bool,
            transparency: i32,
            terminal_font: &QFont,
            cell_height: i32,
        );
        fn rk_font_rendering(font: &mut QFont, anti_alias: bool, hinting: bool);
        fn rk_font_family(font: &mut QFont, family: &QString);
        /// A color from `#rgb` notation or a name as `#rrggbb`, or an empty string.
        fn rk_color_from_text(text: &QString) -> QString;
        fn rk_clipboard_set(text: &QString, selection: bool);
        fn rk_clipboard_get(selection: bool) -> QString;
        fn rk_clipboard_set_rich(text: &QString, html: &QString);
        /// Whether a window of the application, such as a menu or dialog, has the focus.
        fn rk_application_has_focus() -> bool;
        fn rk_system_fonts_json() -> QString;
        fn rk_fixed_fonts() -> QString;
        /// Asks without blocking for a file to save to; the answer arrives in
        /// `qt_file_chosen` with the same `purpose`.
        fn rk_ask_save_file(terminal: i32, purpose: i32, suggestion: &QString);
        /// Prints after the print dialog, which does not block.
        fn rk_print_text(text: &QString);
        /// Asks without blocking for the scrollback size of a terminal; the answer arrives in
        /// `qt_history_lines_chosen`.
        fn rk_ask_history_lines(terminal: i32, current: i32, title: &QString, label: &QString);

        /// The names of the screens, separated by commas.
        fn rk_screen_names() -> QString;
        /// The screen under the mouse pointer, if Qt knows where it is.
        fn rk_pointer_screen() -> QString;
        fn rk_primary_screen() -> QString;
        /// `width,height` of what the task bars leave free on a screen.
        fn rk_screen_available_size(name: &QString) -> QString;
        /// The colors of `theme::parse_palette`, from the palette of Qt.
        fn rk_palette_colors() -> QString;
        /// Opens a link or a file with what the system has for it. The caller checks it with
        /// `links::may_open` first.
        fn rk_open_url_with_qt(url: &QString);
        /// The languages of the user, the preferred first, separated by commas.
        fn rk_ui_languages() -> QString;
        /// A key with modifiers, as from a key event of QML, in the portable text of Qt.
        fn rk_key_sequence_text(key: i32, modifiers: i32) -> QString;
        /// A sequence in portable text as the user's language names its keys.
        fn rk_native_key_sequence(portable: &QString) -> QString;
    }

    extern "Rust" {
        fn qt_dialog_active(active: bool);
        fn qt_history_lines_chosen(terminal: i32, lines: i32);
        fn qt_file_chosen(terminal: i32, purpose: i32, path: &QString);
    }
}

pub use ffi::*;

/// Shorthand for building a `QString`.
#[must_use]
pub fn qs(text: &str) -> QString {
    QString::from(text)
}

/// Runs directly: a dialog opens right after this call.
fn qt_dialog_active(active: bool) {
    crate::controller::with_controller(|controller| controller.dialog_active(active));
}

/// Queued: the dialog closes from within the event loop of Qt.
fn qt_history_lines_chosen(terminal: i32, lines: i32) {
    crate::controller::queue(move |controller| controller.history_lines_chosen(terminal, lines));
}

/// Queued: the dialog closes from within the event loop of Qt.
fn qt_file_chosen(terminal: i32, purpose: i32, path: &QString) {
    let path = std::path::PathBuf::from(path.to_string());
    crate::controller::queue(move |controller| controller.file_chosen(terminal, purpose, path));
}
