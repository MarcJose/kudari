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
        include!("cxx-qt-lib/qcolor.h");
        type QColor = cxx_qt_lib::QColor;
        include!("cxx-qt-lib/qpointf.h");
        type QPointF = cxx_qt_lib::QPointF;
        include!(<QtGui/QRawFont>);
        /// A font that turns characters into glyphs without the text layout of `QPainter`.
        type QRawFont;
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
        /// For the GUI test: paints each terminal that shows anew, and saves what the window
        /// shows in its place; nothing without the test hooks.
        fn rk_test_paint_terminals(path: &QString);
        fn rk_raw_font(painter: &QPainter, font: &QFont) -> UniquePtr<QRawFont>;
        /// The glyph of `character` in `font`, or 0 when the font has none.
        fn rk_glyph_index(font: &QRawFont, character: u32) -> u32;
        /// Draws each glyph with its point on the baseline, all in one color.
        fn rk_draw_glyphs(
            painter: Pin<&mut QPainter>,
            font: &QRawFont,
            color: &QColor,
            glyphs: &[u32],
            positions: &[QPointF],
        );
        /// Draws `text` from a point on the baseline that need not be on a whole pixel.
        fn rk_draw_text(painter: Pin<&mut QPainter>, position: &QPointF, text: &QString);
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
        /// The key of a key event without the modifiers that change what it types, from the
        /// layout of the keyboard: `Qt::Key_7` for Ctrl+Shift+7 on a German keyboard, which
        /// Qt names `Qt::Key_Slash`. The key itself where the platform cannot tell.
        fn rk_base_key(key: i32, modifiers: i32, scan_code: i32, text: &QString) -> i32;
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

fn qt_dialog_active(active: bool) {
    if active {
        crate::controller::dialog_opened();
    } else {
        crate::controller::dialog_finished();
    }
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
