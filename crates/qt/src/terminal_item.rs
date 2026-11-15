// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! QML item that draws one terminal with `QPainter` and handles its input.

#![allow(
    clippy::must_use_candidate,
    clippy::missing_safety_doc,
    reason = "cxx-qt generates public functions without these attributes"
)]
// The bridge compares `f64` properties to notice changes; an exact compare is right there.
#![allow(clippy::float_cmp)]

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{ClearMode, Handler, Rgb};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QColor, QFont, QPen, QPoint, QPointF, QRectF, QString, QVariant};

use crate::colors;
use crate::frontend::frontend;
use crate::input::{Modifiers, prepare_paste, would_execute};
use crate::qt as kde;
use crate::qt_input;
use crate::render::{self, FontStyle, Paint};
use crate::services::Selection;
use crate::sessions::TerminalId;
use crate::terminal;
use crate::view::{self, Context, Effect, View};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qfont.h");
        type QFont = cxx_qt_lib::QFont;
        include!("cxx-qt-lib/qpainter.h");
        type QPainter = cxx_qt_lib::QPainter;
        include!("cxx-qt-lib/qsizef.h");
        type QSizeF = cxx_qt_lib::QSizeF;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    /// The queries this item answers; the other values of the Qt enum stay unnamed.
    #[namespace = "Qt"]
    #[repr(i32)]
    enum InputMethodQuery {
        ImEnabled = 1,
        ImCursorRectangle = 2,
    }

    #[namespace = "Qt"]
    unsafe extern "C++" {
        include!(<QtCore/qnamespace.h>);
        type InputMethodQuery;
    }

    unsafe extern "C++" {
        include!(<QtGui/QInputMethodEvent>);
        type QInputMethodEvent;
        include!("qtbridge.h");

        unsafe fn rk_enable_input_method(item: *mut TerminalItem);
        unsafe fn rk_input_commit_string(event: *const QInputMethodEvent) -> QString;
        unsafe fn rk_input_preedit_string(event: *const QInputMethodEvent) -> QString;
    }

    unsafe extern "C++" {
        include!(<QtQuick/QQuickPaintedItem>);
        type QQuickPaintedItem;
    }

    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QQuickPaintedItem]
        #[qproperty(i32, terminal_id, cxx_name = "terminalId")]
        #[qproperty(QFont, term_font, cxx_name = "termFont")]
        #[qproperty(f64, cell_width, cxx_name = "cellWidth")]
        #[qproperty(f64, cell_height, cxx_name = "cellHeight")]
        #[qproperty(f64, ascent)]
        #[qproperty(bool, focused)]
        #[qproperty(bool, keyboard_enabled, cxx_name = "keyboardEnabled")]
        #[qproperty(bool, opaque)]
        #[qproperty(bool, bold_intense, cxx_name = "boldIntense")]
        #[qproperty(bool, auto_copy, cxx_name = "autoCopy")]
        #[qproperty(bool, alternate_scrolling, cxx_name = "alternateScrolling")]
        #[qproperty(QString, middle_click_paste, cxx_name = "middleClickPaste")]
        #[qproperty(i32, columns)]
        #[qproperty(i32, lines)]
        #[qproperty(bool, cursor_hidden, cxx_name = "cursorHidden")]
        #[qproperty(f64, cursor_fade, cxx_name = "cursorFade")]
        #[qproperty(bool, url_hints, cxx_name = "urlHints")]
        #[qproperty(QString, search_lines, cxx_name = "searchLines")]
        #[qproperty(f64, scroll_mark_y, cxx_name = "scrollMarkY")]
        #[qproperty(f64, scroll_mark_height, cxx_name = "scrollMarkHeight")]
        #[qproperty(i32, scroll_mark_serial, cxx_name = "scrollMarkSerial")]
        #[qproperty(bool, cursor_blinks, cxx_name = "cursorBlinks")]
        #[qproperty(bool, selection_mode, cxx_name = "selectionMode")]
        #[qproperty(f64, scroll_size, cxx_name = "scrollSize")]
        #[qproperty(f64, scroll_position, cxx_name = "scrollPosition")]
        #[qproperty(QString, preedit)]
        #[qproperty(f64, cursor_x, cxx_name = "cursorX")]
        #[qproperty(f64, cursor_y, cxx_name = "cursorY")]
        type TerminalItem = super::TerminalItemRust;

        #[qinvokable]
        #[cxx_override]
        unsafe fn paint(self: Pin<&mut Self>, painter: *mut QPainter);

        #[cxx_override]
        unsafe fn input_method_event(self: Pin<&mut Self>, event: *mut QInputMethodEvent);

        #[cxx_override]
        fn input_method_query(self: &Self, query: InputMethodQuery) -> QVariant;

        #[inherit]
        fn size(self: &Self) -> QSizeF;

        #[inherit]
        fn update(self: Pin<&mut Self>);

        /// Applies a new pixel size to the grid.
        #[qinvokable]
        fn resize_to(self: Pin<&mut Self>, width: f64, height: f64);

        /// Re-reads scroll state after new output and repaints.
        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);

        /// Handles a key. Returns false if a blocked key should be indicated.
        #[qinvokable]
        fn key_press(self: Pin<&mut Self>, key: i32, modifiers: i32, text: &QString) -> bool;

        #[qinvokable]
        fn mouse_press(self: Pin<&mut Self>, x: f64, y: f64, button: i32, clicks: i32, modifiers: i32);

        #[qinvokable]
        fn mouse_move(self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32);

        #[qinvokable]
        fn mouse_release(self: Pin<&mut Self>, x: f64, y: f64, button: i32, modifiers: i32);

        #[qinvokable]
        fn wheel(self: Pin<&mut Self>, delta: i32, x: f64, y: f64);

        #[qinvokable]
        fn scroll_to_position(self: Pin<&mut Self>, position: f64);

        #[qinvokable]
        fn has_selection(self: &Self) -> bool;

        #[qinvokable]
        fn copy_selection(self: Pin<&mut Self>);

        #[qinvokable]
        fn paste(self: Pin<&mut Self>, selection: bool);

        #[qinvokable]
        fn select_all(self: Pin<&mut Self>);

        /// Selects the next match of a regular expression and scrolls to it.
        #[qinvokable]
        fn find(
            self: Pin<&mut Self>,
            pattern: &QString,
            backwards: bool,
            case_sensitive: bool,
            regular_expression: bool,
            highlight_all: bool,
        ) -> bool;

        #[qinvokable]
        fn end_search(self: Pin<&mut Self>);

        /// Scrolls during a selection that left the terminal.
        #[qinvokable]
        fn scroll_by(self: Pin<&mut Self>, lines: i32);

        #[qinvokable]
        fn select_line(self: Pin<&mut Self>);

        /// Starts or ends selecting with the keyboard.
        #[qinvokable]
        fn toggle_selection_mode(self: Pin<&mut Self>);

        /// Pastes dropped text, or shell-quoted paths for a newline-separated list of files.
        #[qinvokable]
        fn drop_text(self: Pin<&mut Self>, text: &QString, are_files: bool);

        /// `x,y,width` of the link at a position, in item coordinates, or an empty string.
        #[qinvokable]
        fn link_geometry(self: &Self, x: f64, y: f64, files: bool) -> QString;

        #[qinvokable]
        fn clear_selection(self: Pin<&mut Self>);

        #[qinvokable]
        fn clear_scrollback(self: Pin<&mut Self>);

        /// The link at a position, or an empty string.
        #[qinvokable]
        fn link_at(self: &Self, x: f64, y: f64) -> QString;

        /// A color named by the word at a position, as `#rrggbb`, or an empty string.
        #[qinvokable]
        fn color_at(self: &Self, x: f64, y: f64) -> QString;

        /// Whether the selection covers a position.
        #[qinvokable]
        fn selection_contains(self: &Self, x: f64, y: f64) -> bool;

        /// The selection as text, for dragging it away.
        #[qinvokable]
        fn selection_text(self: &Self) -> QString;
    }

    impl cxx_qt::Initialize for TerminalItem {}
    impl cxx_qt::Threading for TerminalItem {}
}

impl cxx_qt::Initialize for qobject::TerminalItem {
    fn initialize(mut self: Pin<&mut Self>) {
        // SAFETY: the pointer is only used during the call.
        unsafe { qobject::rk_enable_input_method(self.as_mut().get_unchecked_mut()) };
        self.as_mut()
            .on_focused_changed(|mut item| {
                item.as_mut().report_focus();
                item.update();
            })
            .release();
        self.as_mut()
            .on_cursor_hidden_changed(qobject::TerminalItem::update)
            .release();
        self.as_mut()
            .on_cursor_fade_changed(qobject::TerminalItem::update)
            .release();
        self.on_url_hints_changed(qobject::TerminalItem::update).release();
    }
}

pub struct TerminalItemRust {
    terminal_id: i32,
    term_font: QFont,
    preedit: QString,
    cell_width: f64,
    cell_height: f64,
    ascent: f64,
    focused: bool,
    keyboard_enabled: bool,
    opaque: bool,
    bold_intense: bool,
    auto_copy: bool,
    alternate_scrolling: bool,
    middle_click_paste: QString,
    columns: i32,
    lines: i32,
    cursor_hidden: bool,
    cursor_fade: f64,
    url_hints: bool,
    search_lines: QString,
    scroll_mark_y: f64,
    scroll_mark_height: f64,
    scroll_mark_serial: i32,
    cursor_blinks: bool,
    selection_mode: bool,
    scroll_size: f64,
    scroll_position: f64,
    cursor_x: f64,
    cursor_y: f64,
    /// Selection, search and the mouse, in the core.
    view: View,
    /// Where the left button went down, to tell a click from a selection.
    press_position: Option<(f64, f64)>,
    /// The words on the screen that the link numbers were found in.
    hint_words: Vec<view::HintWord>,
    /// Counts the looks for files among them, so that an old one is dropped.
    hint_generation: u64,
    /// A thread looks for files; on a drive that hangs it may never come back.
    hint_looking: Arc<AtomicBool>,
}

impl Default for TerminalItemRust {
    fn default() -> Self {
        Self {
            terminal_id: -1,
            term_font: QFont::default(),
            preedit: QString::default(),
            cell_width: 1.0,
            cell_height: 1.0,
            ascent: 0.0,
            focused: false,
            keyboard_enabled: true,
            opaque: true,
            bold_intense: true,
            auto_copy: true,
            alternate_scrolling: true,
            middle_click_paste: QString::from("selection"),
            columns: 0,
            lines: 0,
            cursor_hidden: false,
            cursor_fade: 1.0,
            url_hints: false,
            search_lines: QString::default(),
            scroll_mark_y: 0.0,
            scroll_mark_height: 0.0,
            scroll_mark_serial: 0,
            cursor_blinks: false,
            selection_mode: false,
            scroll_size: 1.0,
            scroll_position: 0.0,
            cursor_x: 0.0,
            cursor_y: 0.0,
            view: View::default(),
            press_position: None,
            hint_words: Vec::new(),
            hint_generation: 0,
            hint_looking: Arc::default(),
        }
    }
}

/// How long the files among the words of the link numbers may take to look up.
const HINT_FILES_TIME: Duration = Duration::from_millis(100);

/// Types text from the clipboard or a drop into a terminal as its shell expects pasted text:
/// in brackets where it understands them, else after asking if the text would run commands.
pub fn paste_into(terminal: TerminalId, text: String) {
    let Some(backend) = terminal::backend(terminal) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let bracketed = backend.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
    // Without bracketed paste the shell runs every line as soon as it arrives. The question
    // goes through the queue, since this can run inside a call of the controller.
    if !bracketed && would_execute(&text) {
        crate::controller::queue(move |controller| controller.ask_paste(terminal, text));
        return;
    }
    backend.write_user_input(prepare_paste(&text, bracketed).into_bytes());
}

/// What a drop on a terminal types: dropped text as pasted text, whose lines must not run
/// unasked, or the dropped files, a path on each line, quoted for the shell of the terminal;
/// a path it cannot take safely is left out.
pub fn drop_into(terminal: TerminalId, text: String, are_files: bool) {
    if !are_files {
        paste_into(terminal, text);
        return;
    }
    let Some(backend) = terminal::backend(terminal) else {
        return;
    };
    let quoted = text
        .lines()
        .filter_map(|path| backend.quote_path(path))
        .collect::<Vec<_>>()
        .join(" ");
    if !quoted.is_empty() {
        backend.write_user_input(quoted.into_bytes());
    }
}

/// The size of a terminal item and of its cells.
#[derive(Clone, Copy)]
struct Cells {
    width: f64,
    height: f64,
    cell_width: f64,
    cell_height: f64,
}

impl Cells {
    /// The cell at item coordinates, and the side within it. It has to come from the same
    /// lock of `term` as what reads the cell: in between, the program can switch to the
    /// alternate screen or clear the scrollback, and the point would be off the grid.
    fn at<T>(self, term: &Term<T>, x: f64, y: f64) -> (Point, Side) {
        let (columns, lines) = (term.columns(), term.screen_lines());
        let (origin_x, origin_y) = terminal::text_origin(
            self.width,
            self.height,
            columns as f64 * self.cell_width,
            lines as f64 * self.cell_height,
        );
        let (x, y) = (x - origin_x, y - origin_y);
        let column = ((x / self.cell_width).floor().max(0.0) as usize).min(columns - 1);
        let line = ((y / self.cell_height).floor().max(0.0) as i32).min(lines as i32 - 1);
        let side = if x / self.cell_width - (column as f64) < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        let offset = term.grid().display_offset() as i32;
        (Point::new(Line(line - offset), Column(column)), side)
    }
}

impl qobject::TerminalItem {
    fn backend(&self) -> Option<std::sync::Arc<terminal::Backend>> {
        terminal::backend(*self.terminal_id())
    }

    /// The size of the item and of its cells, to find cells under a terminal locked later.
    fn cells(&self) -> Cells {
        let size = self.size();
        Cells {
            width: size.width(),
            height: size.height(),
            cell_width: *self.cell_width(),
            cell_height: *self.cell_height(),
        }
    }

    /// Top left corner of the grid in item coordinates.
    fn text_origin(&self, columns: usize, lines: usize) -> (f64, f64) {
        let size = self.size();
        terminal::text_origin(
            size.width(),
            size.height(),
            columns as f64 * *self.cell_width(),
            lines as f64 * *self.cell_height(),
        )
    }

    fn grid_for(&self, width: f64, height: f64) -> (u16, u16) {
        // The item excludes the scrollbar, which `grid_size` expects to be included.
        terminal::grid_size(
            (width + terminal::scrollbar_space()) as i32,
            height as i32,
            *self.cell_width(),
            *self.cell_height(),
        )
    }

    fn resize_to(mut self: Pin<&mut Self>, width: f64, height: f64) {
        let (columns, lines) = self.as_ref().grid_for(width, height);
        self.as_mut().set_columns(columns.into());
        self.as_mut().set_lines(lines.into());
        if let Some(backend) = self.as_ref().backend() {
            backend.resize(columns, lines, *self.cell_width() as u16, *self.cell_height() as u16);
        }
        self.refresh();
    }

    fn refresh(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            let term = backend.term.lock();
            let history = term.grid().history_size() as f64;
            let lines = term.screen_lines() as f64;
            let offset = term.grid().display_offset() as f64;
            let cursor = term.grid().cursor.point;
            let blinks = term.cursor_style().blinking;
            let selection_mode = view::selection_mode(&term);
            let (origin_x, origin_y) = self.as_ref().text_origin(term.columns(), term.screen_lines());
            drop(term);
            self.as_mut().set_cursor_blinks(blinks);
            self.as_mut().set_selection_mode(selection_mode);
            self.as_mut().update_matches();
            let total = history + lines;
            self.as_mut().set_scroll_size(lines / total);
            self.as_mut().set_scroll_position((history - offset) / total);
            let x = origin_x + cursor.column.0 as f64 * *self.cell_width();
            let y = origin_y + (f64::from(cursor.line.0) + offset) * *self.cell_height();
            self.as_mut().set_cursor_x(x);
            self.as_mut().set_cursor_y(y);
        }
        self.update();
    }

    /// Runs `action` on the view with the locked terminal, then carries out its effects.
    fn handle(
        mut self: Pin<&mut Self>,
        action: impl FnOnce(&mut View, &mut Term<terminal::Proxy>, &Context) -> Vec<Effect>,
    ) {
        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let options = terminal::options().unwrap_or_default();
        let directory = || backend.working_directory();
        let context = Context {
            options: &options,
            tracks_mouse: backend.tracks_mouse(),
            directory: &directory,
        };
        let effects = {
            let mut term = backend.term.lock();
            action(&mut self.as_mut().rust_mut().view, &mut term, &context)
        };
        self.apply(effects);
    }

    fn apply(mut self: Pin<&mut Self>, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Type(bytes) => {
                    if let Some(backend) = self.as_ref().backend() {
                        backend.write_user_input(bytes);
                    }
                }
                Effect::Write(bytes) => {
                    if let Some(backend) = self.as_ref().backend() {
                        backend.write(bytes);
                    }
                }
                Effect::OpenLink(link) => terminal::open_link(&link),
                Effect::Copy(text) => frontend().copy(&text, Selection::Clipboard),
                Effect::CopyPrimary(text) => frontend().copy(&text, Selection::Primary),
                Effect::Paste { selection } => self.as_mut().paste(selection),
                Effect::Scroll(lines) => self.as_mut().scroll_lines(lines),
                Effect::SelectionMode(active) => self.as_mut().set_selection_mode(active),
                Effect::Hints(shown) => self.as_mut().set_url_hints(shown),
                Effect::Refresh => self.as_mut().refresh(),
                Effect::Repaint => self.as_mut().update(),
            }
        }
    }

    fn key_press(self: Pin<&mut Self>, key: i32, modifiers: i32, text: &QString) -> bool {
        let (key, modifiers) = (qt_input::key(key), qt_input::modifiers(modifiers));
        if !*self.keyboard_enabled() {
            return modifiers.intersects(Modifiers::SHIFT | Modifiers::ALT | Modifiers::CONTROL);
        }
        let text = text.to_string();
        self.handle(|view, term, context| view.key(term, key, modifiers, &text, context));
        true
    }

    /// # Safety
    ///
    /// `event` must be valid for the duration of the call, which Qt guarantees.
    pub unsafe fn input_method_event(mut self: Pin<&mut Self>, event: *mut qobject::QInputMethodEvent) {
        // SAFETY: Qt keeps the event valid during this call.
        let (commit, preedit) = unsafe {
            (
                qobject::rk_input_commit_string(event),
                qobject::rk_input_preedit_string(event),
            )
        };
        if !commit.is_empty() {
            self.as_mut().commit_text(&commit);
        }
        self.set_preedit(preedit);
    }

    fn input_method_query(&self, query: qobject::InputMethodQuery) -> QVariant {
        match query {
            qobject::InputMethodQuery::ImEnabled => QVariant::from(&true),
            qobject::InputMethodQuery::ImCursorRectangle => QVariant::from(&QRectF::new(
                *self.cursor_x(),
                *self.cursor_y(),
                *self.cell_width(),
                *self.cell_height(),
            )),
            _ => QVariant::default(),
        }
    }

    /// Tells programs such as Vim and tmux about focus changes, if they asked for it.
    fn report_focus(self: Pin<&mut Self>) {
        let focused = *self.focused();
        if let Some(backend) = self.as_ref().backend() {
            let mut term = backend.term.lock();
            term.is_focused = focused;
            let wanted = term.mode().contains(TermMode::FOCUS_IN_OUT);
            drop(term);
            if wanted {
                backend.write(if focused {
                    b"\x1b[I".to_vec()
                } else {
                    b"\x1b[O".to_vec()
                });
            }
        }
    }

    fn toggle_selection_mode(self: Pin<&mut Self>) {
        self.handle(|_, term, _| view::toggle_selection_mode(term));
    }

    fn scroll_by(mut self: Pin<&mut Self>, lines: i32) {
        if let Some(backend) = self.as_ref().backend() {
            backend.term.lock().scroll_display(Scroll::Delta(lines));
        }
        self.as_mut().refresh();
    }

    fn drop_text(self: Pin<&mut Self>, text: &QString, are_files: bool) {
        if !*self.keyboard_enabled() {
            return;
        }
        drop_into(*self.terminal_id(), text.to_string(), are_files);
    }

    fn commit_text(mut self: Pin<&mut Self>, text: &QString) {
        if !*self.keyboard_enabled() {
            return;
        }
        if let Some(backend) = self.as_ref().backend() {
            backend.term.lock().selection = None;
            backend.write_user_input(text.to_string().into_bytes());
        }
        self.as_mut().refresh();
    }

    fn mouse_press(mut self: Pin<&mut Self>, x: f64, y: f64, button: i32, clicks: i32, modifiers: i32) {
        let (button, modifiers) = (qt_input::button(button), qt_input::modifiers(modifiers));
        let cells = self.as_ref().cells();
        let middle_paste = match self.middle_click_paste().to_string().as_str() {
            "none" => None,
            source => Some(source == "selection"),
        };
        self.as_mut().handle(|view, term, context| {
            let cell = cells.at(term, x, y);
            view.press(term, cell, button, clicks, modifiers, middle_paste, context)
        });
        if self.view.selecting() {
            self.as_mut().rust_mut().press_position = Some((x, y));
        }
    }

    fn mouse_move(self: Pin<&mut Self>, x: f64, y: f64, modifiers: i32) {
        let modifiers = qt_input::modifiers(modifiers);
        let cells = self.as_ref().cells();
        self.handle(|view, term, context| {
            let cell = cells.at(term, x, y);
            view.motion(term, cell, modifiers, context)
        });
    }

    fn mouse_release(mut self: Pin<&mut Self>, x: f64, y: f64, button: i32, modifiers: i32) {
        let (button, modifiers) = (qt_input::button(button), qt_input::modifiers(modifiers));
        let cells = self.as_ref().cells();
        let press = self.as_mut().rust_mut().press_position.take();
        let clicked = press.is_some_and(|(press_x, press_y)| (press_x - x).abs() < 3.0 && (press_y - y).abs() < 3.0);
        let auto_copy = *self.auto_copy();
        self.handle(|view, term, context| {
            let point = Some(cells.at(term, x, y).0);
            view.release(term, point, button, modifiers, clicked, auto_copy, context)
        });
    }

    fn wheel(self: Pin<&mut Self>, delta: i32, x: f64, y: f64) {
        let cells = self.as_ref().cells();
        let alternate_scrolling = *self.alternate_scrolling();
        self.handle(|view, term, context| {
            let point = Some(cells.at(term, x, y).0);
            view.wheel(term, point, delta > 0, alternate_scrolling, context)
        });
    }

    /// Scrolls the view and marks the lines that came in for a moment.
    fn scroll_lines(mut self: Pin<&mut Self>, lines: i32) {
        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let mut term = backend.term.lock();
        let moved = view::scroll(&mut term, lines);
        let screen_lines = term.screen_lines() as i32;
        let (_, origin_y) = self.as_ref().text_origin(term.columns(), term.screen_lines());
        drop(term);
        if moved != 0 {
            let count = moved.abs().min(screen_lines);
            let first = if moved > 0 { 0 } else { screen_lines - count };
            let cell_height = *self.cell_height();
            self.as_mut()
                .set_scroll_mark_y(origin_y + f64::from(first) * cell_height);
            self.as_mut().set_scroll_mark_height(f64::from(count) * cell_height);
            let serial = self.scroll_mark_serial().wrapping_add(1);
            self.as_mut().set_scroll_mark_serial(serial);
        }
        self.as_mut().refresh();
    }

    fn scroll_to_position(mut self: Pin<&mut Self>, position: f64) {
        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let term = backend.term.lock();
        let history = term.grid().history_size() as f64;
        let total = history + term.screen_lines() as f64;
        let offset = (history - position * total).round() as i32;
        let current = term.grid().display_offset() as i32;
        drop(term);
        self.as_mut().scroll_lines(offset - current);
    }

    fn has_selection(&self) -> bool {
        self.backend()
            .is_some_and(|backend| backend.term.lock().selection.as_ref().is_some_and(|s| !s.is_empty()))
    }

    fn copy_selection(mut self: Pin<&mut Self>) {
        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let options = terminal::options().unwrap_or_default();
        let mut term = backend.term.lock();
        let Some(text) = view::selected_text(&term, &options) else {
            return;
        };
        let html = if options.copy_html {
            view::selection_html(&term)
        } else {
            String::new()
        };
        frontend().copy_rich(&text, &html);
        if options.clear_on_copy {
            term.selection = None;
            drop(term);
            self.as_mut().update();
        }
    }

    fn paste(self: Pin<&mut Self>, selection: bool) {
        if !*self.keyboard_enabled() {
            return;
        }
        let source = if selection {
            Selection::Primary
        } else {
            Selection::Clipboard
        };
        paste_into(*self.terminal_id(), frontend().paste(source));
    }

    #[allow(
        clippy::fn_params_excessive_bools,
        reason = "called from QML, which has no enums for this"
    )]
    fn find(
        mut self: Pin<&mut Self>,
        pattern: &QString,
        backwards: bool,
        case_sensitive: bool,
        regular_expression: bool,
        highlight_all: bool,
    ) -> bool {
        let Some(backend) = self.as_ref().backend() else {
            return false;
        };
        let found = {
            let mut term = backend.term.lock();
            let pattern = pattern.to_string();
            let view = &mut self.as_mut().rust_mut().view;
            view.find(
                &mut term,
                &pattern,
                backwards,
                case_sensitive,
                regular_expression,
                highlight_all,
            )
        };
        self.as_mut().refresh();
        self.as_mut().update_search_lines();
        found
    }

    /// Finds all matches on screen for the "highlight all" option of the search.
    fn update_matches(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            let term = backend.term.lock();
            self.as_mut().rust_mut().view.update_matches(&term);
        }
    }

    /// Publishes where the matches lie in the scrollback, for the scrollbar.
    fn update_search_lines(mut self: Pin<&mut Self>) {
        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let lines = {
            let term = backend.term.lock();
            self.as_mut().rust_mut().view.search_lines(&term)
        };
        let json = lines.map(|lines| serde_json::to_string(&lines).unwrap_or_default());
        self.as_mut().set_search_lines(kde::qs(&json.unwrap_or_default()));
    }

    fn end_search(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            let mut term = backend.term.lock();
            self.as_mut().rust_mut().view.end_search(&mut term);
        }
        self.as_mut().set_search_lines(QString::default());
        self.as_mut().update();
    }

    fn clear_selection(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            backend.term.lock().selection = None;
        }
        self.as_mut().update();
    }

    fn clear_scrollback(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            backend.term.lock().clear_screen(ClearMode::Saved);
        }
        self.as_mut().refresh();
    }

    /// The link under a point of the item.
    fn link(&self, x: f64, y: f64, files: bool) -> Option<view::Link> {
        let backend = self.backend()?;
        let options = terminal::options().unwrap_or_default();
        let directory = || backend.working_directory();
        let context = Context {
            options: &options,
            tracks_mouse: backend.tracks_mouse(),
            directory: &directory,
        };
        let term = backend.term.lock();
        let (point, _) = self.cells().at(&term, x, y);
        view::link_at(&term, point, files, &context)
    }

    fn color_at(&self, x: f64, y: f64) -> QString {
        let Some(backend) = self.backend() else {
            return QString::default();
        };
        let term = backend.term.lock();
        let (point, _) = self.cells().at(&term, x, y);
        let word = view::color_word_at(&term, point);
        drop(term);
        word.map_or_else(QString::default, |word| kde::rk_color_from_text(&kde::qs(&word)))
    }

    fn selection_contains(&self, x: f64, y: f64) -> bool {
        self.backend().is_some_and(|backend| {
            let term = backend.term.lock();
            view::selection_contains(&term, self.cells().at(&term, x, y).0)
        })
    }

    fn selection_text(&self) -> QString {
        let options = terminal::options().unwrap_or_default();
        self.backend()
            .and_then(|backend| view::selected_text(&backend.term.lock(), &options))
            .map_or_else(QString::default, |text| kde::qs(&text))
    }

    fn link_at(&self, x: f64, y: f64) -> QString {
        self.link(x, y, true)
            .map_or_else(QString::default, |link| kde::qs(&link.target))
    }

    fn link_geometry(&self, x: f64, y: f64, files: bool) -> QString {
        let Some((link, backend)) = self.link(x, y, files).zip(self.backend()) else {
            return QString::default();
        };
        let term = backend.term.lock();
        let offset = term.grid().display_offset() as i32;
        let (origin_x, origin_y) = self.text_origin(term.columns(), term.screen_lines());
        drop(term);
        let (cell_width, cell_height) = (*self.cell_width(), *self.cell_height());
        kde::qs(&format!(
            "{},{},{}",
            origin_x + link.start.column.0 as f64 * cell_width,
            origin_y + f64::from(link.start.line.0 + offset + 1) * cell_height - 1.0,
            link.cells as f64 * cell_width,
        ))
    }

    fn select_line(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            view::select_line(&mut backend.term.lock());
        }
        self.as_mut().update();
    }

    fn select_all(mut self: Pin<&mut Self>) {
        if let Some(backend) = self.as_ref().backend() {
            view::select_all(&mut backend.term.lock());
        }
        self.as_mut().update();
    }

    /// # Safety
    ///
    /// `painter` must be null or valid for the duration of the call, which Qt guarantees.
    pub unsafe fn paint(mut self: Pin<&mut Self>, painter: *mut qobject::QPainter) {
        // SAFETY: Qt passes null or a painter that is valid during this call and stays in place.
        let Some(painter) = (unsafe { painter.as_mut() }) else {
            return;
        };
        // SAFETY: Qt keeps the painter in place during this call.
        let mut target = unsafe { Pin::new_unchecked(painter) };
        // Text draws much faster into the image of `rk_frame_begin` than into the item's own.
        let opaque = *self.opaque();
        let mut frame = kde::rk_frame_begin(target.as_mut(), opaque);
        let drawn = if let Some(inner) = frame.as_mut() {
            self.as_mut().paint_frame(kde::rk_frame_painter(inner));
            true
        } else {
            false
        };
        if drawn {
            kde::rk_frame_end(target, frame, opaque);
        } else {
            self.paint_frame(target);
        }
    }

    /// Looks for files among the words of the link numbers on a thread of its own, without
    /// the lock of the terminal: on a slow or hanging network drive, a look can take seconds
    /// or never end, and the window, the drawing and the program in the terminal must not
    /// wait for it.
    fn look_for_hint_files(mut self: Pin<&mut Self>, words: Vec<view::HintWord>, backend: Arc<terminal::Backend>) {
        if self.hint_looking.swap(true, Ordering::AcqRel) {
            // Still looking for older words: the next frame after it ends tries again.
            self.as_mut().rust_mut().hint_words.clear();
            return;
        }
        let generation = self.hint_generation + 1;
        self.as_mut().rust_mut().hint_generation = generation;
        let looking = Arc::clone(&self.hint_looking);
        let thread = self.qt_thread();
        let started = std::thread::Builder::new()
            .name("kudari-hints".to_string())
            .spawn(move || {
                let directory = backend.working_directory();
                let deadline = Instant::now() + HINT_FILES_TIME;
                let links = view::hint_links(&words, Some(deadline), |word| {
                    directory
                        .as_deref()
                        .and_then(|directory| view::existing_file(word, directory))
                });
                looking.store(false, Ordering::Release);
                let _ = thread.queue(move |mut item| {
                    if item.hint_generation == generation && *item.url_hints() {
                        item.as_mut().rust_mut().view.set_hints(links);
                    }
                    item.update();
                });
            });
        if started.is_err() {
            self.hint_looking.store(false, Ordering::Release);
        }
    }

    fn paint_frame(mut self: Pin<&mut Self>, mut painter: Pin<&mut qobject::QPainter>) {
        let scheme = colors::active();
        // A translucent background comes from the pane behind this item.
        if *self.opaque() {
            let size = self.size();
            painter.as_mut().fill_rect(
                &QRectF::new(0.0, 0.0, size.width(), size.height()),
                &qcolor(scheme.background),
            );
        }

        let Some(backend) = self.as_ref().backend() else {
            return;
        };
        let options = terminal::options().unwrap_or_default();
        // Read before locking the terminal: this looks at the running processes.
        let badge = options
            .badge
            .as_ref()
            .map(|badge| (backend.local_title(&badge.format), badge));
        let fonts = Fonts::new(self.term_font(), &options);
        let (paints, fresh_words) = {
            let term = backend.term.lock();
            // Only the words under the lock; whether they name files is looked up later.
            let fresh_words = if *self.url_hints() {
                Some(view::hint_words(&term)).filter(|words| *words != self.hint_words)
            } else {
                None
            };
            if let Some(words) = &fresh_words {
                // Web addresses at once, the files when the thread has found them.
                let urls = view::hint_links(words, Some(Instant::now()), |_| None);
                let mut item = self.as_mut().rust_mut();
                item.view.set_hints(urls);
                item.hint_words.clone_from(words);
            } else if !*self.url_hints() && !self.hint_words.is_empty() {
                let mut item = self.as_mut().rust_mut();
                item.hint_words.clear();
                item.view.set_hints(Vec::new());
            }
            let (origin_x, origin_y) = self.as_ref().text_origin(term.columns(), term.screen_lines());
            let scene = render::Scene {
                geometry: render::Geometry {
                    origin_x,
                    origin_y,
                    cell_width: *self.cell_width(),
                    cell_height: *self.cell_height(),
                    ascent: *self.ascent(),
                },
                scheme: &scheme,
                bold_intense: *self.bold_intense(),
                focused: *self.focused(),
                cursor_hidden: *self.cursor_hidden(),
                cursor_fade: *self.cursor_fade(),
                hints: self.view.hints(),
                matches: self.view.matches(),
            };
            (render::frame(&term, &options, &scene), fresh_words)
        };
        if let Some(words) = fresh_words {
            self.as_mut().look_for_hint_files(words, backend);
        }

        painter.as_mut().set_font(&fonts.regular);
        if let Some((text, badge)) = badge.filter(|(text, _)| !text.is_empty()) {
            kde::rk_draw_badge(
                painter.as_mut(),
                self.size().width(),
                &kde::qs(&text),
                &kde::qs(&badge.color),
                &kde::qs(&badge.family),
                badge.size,
                badge.text_only,
                badge.transparency,
                self.term_font(),
                *self.cell_height() as i32,
            );
        }
        let mut runs = GlyphRuns::default();
        for paint in paints {
            match paint {
                Paint::Fill(rect, color) => {
                    // What comes before a fill stays under it.
                    runs.draw(painter.as_mut());
                    painter
                        .as_mut()
                        .fill_rect(&QRectF::new(rect.x, rect.y, rect.width, rect.height), &qcolor(color));
                }
                Paint::Text {
                    x,
                    y,
                    text,
                    color,
                    font,
                } => {
                    let slot = fonts.slot(font);
                    if !runs.add(&painter, &fonts, slot, &text, (x, y), color) {
                        painter.as_mut().set_font(fonts.get(slot));
                        painter.as_mut().set_pen(&QPen::from(&qcolor(color)));
                        painter
                            .as_mut()
                            .draw_text(&QPoint::new(x, y), &QString::from(text.as_str()));
                    }
                }
            }
        }
        runs.draw(painter.as_mut());
    }
}

/// The glyphs of a frame by font and color, so that each group takes one call of
/// `QPainter::drawGlyphRun` instead of one `drawText` per cell, which costs a text layout each
/// time. Glyphs of one group do not overlap, so their order within it does not matter.
#[derive(Default)]
struct GlyphRuns {
    /// The fonts of `Fonts::get`, made on first use.
    fonts: [Option<cxx::UniquePtr<kde::QRawFont>>; Fonts::SLOTS],
    /// The glyph of each character in each font so far; 0 when the font has none.
    glyphs: std::collections::HashMap<(usize, char), u32>,
    /// The groups in the order of their first glyph, which keeps the drawing the same from
    /// frame to frame.
    runs: Vec<Run>,
    /// The index in `runs` by font and color, as `0xRRGGBB`.
    by_key: std::collections::HashMap<(usize, u32), usize>,
}

struct Run {
    slot: usize,
    color: Rgb,
    glyphs: Vec<u32>,
    positions: Vec<QPointF>,
}

impl GlyphRuns {
    /// Adds a glyph at a point on its baseline. False if `text` is not one character of the
    /// font, such as a character with combining marks or one that only a fallback font has:
    /// those need `drawText`.
    fn add(
        &mut self,
        painter: &qobject::QPainter,
        fonts: &Fonts,
        slot: usize,
        text: &str,
        (x, y): (i32, i32),
        color: Rgb,
    ) -> bool {
        let mut characters = text.chars();
        let (Some(character), None) = (characters.next(), characters.next()) else {
            return false;
        };
        let Some(raw) = self.fonts.get_mut(slot) else {
            return false;
        };
        let Some(raw) = raw
            .get_or_insert_with(|| kde::rk_raw_font(painter, fonts.get(slot)))
            .as_ref()
        else {
            return false;
        };
        let glyph = *self
            .glyphs
            .entry((slot, character))
            .or_insert_with(|| kde::rk_glyph_index(raw, character.into()));
        if glyph == 0 {
            return false;
        }
        let key = (slot, u32::from_be_bytes([0, color.r, color.g, color.b]));
        let index = *self.by_key.entry(key).or_insert_with(|| {
            self.runs.push(Run {
                slot,
                color,
                glyphs: Vec::new(),
                positions: Vec::new(),
            });
            self.runs.len() - 1
        });
        let Some(run) = self.runs.get_mut(index) else {
            return false;
        };
        run.glyphs.push(glyph);
        run.positions.push(QPointF::new(f64::from(x), f64::from(y)));
        true
    }

    /// Draws the glyphs so far and empties the groups.
    fn draw(&mut self, mut painter: Pin<&mut qobject::QPainter>) {
        for run in &mut self.runs {
            if run.glyphs.is_empty() {
                continue;
            }
            let font = self.fonts.get(run.slot).and_then(Option::as_ref);
            if let Some(font) = font.and_then(|font| font.as_ref()) {
                kde::rk_draw_glyphs(painter.as_mut(), font, &qcolor(run.color), &run.glyphs, &run.positions);
            }
            run.glyphs.clear();
            run.positions.clear();
        }
    }
}

struct Fonts {
    emoji: Option<QFont>,
    regular: QFont,
    bold: QFont,
    italic: QFont,
    bold_italic: QFont,
}

impl Fonts {
    fn new(base: &QFont, view: &terminal::Options) -> Self {
        let variant = |bold: bool, italic: bool| {
            let mut font = base.clone();
            kde::rk_font_rendering(&mut font, view.anti_alias, view.hinting);
            font.set_bold(bold);
            font.set_italic(italic);
            font
        };
        let emoji = (!view.emoji_font.is_empty()).then(|| {
            let mut font = variant(false, false);
            kde::rk_font_family(&mut font, &kde::qs(&view.emoji_font));
            font
        });
        Self {
            emoji,
            regular: variant(false, false),
            bold: variant(true, false),
            italic: variant(false, true),
            bold_italic: variant(true, true),
        }
    }

    /// How many fonts `get` tells apart.
    const SLOTS: usize = 5;

    /// The number of the font for `style`, for `get`.
    fn slot(&self, style: FontStyle) -> usize {
        if style.emoji && self.emoji.is_some() {
            4
        } else {
            usize::from(style.bold) + 2 * usize::from(style.italic)
        }
    }

    fn get(&self, slot: usize) -> &QFont {
        match slot {
            1 => &self.bold,
            2 => &self.italic,
            3 => &self.bold_italic,
            4 => self.emoji.as_ref().unwrap_or(&self.regular),
            _ => &self.regular,
        }
    }
}

fn qcolor(rgb: Rgb) -> QColor {
    QColor::from_rgb(rgb.r.into(), rgb.g.into(), rgb.b.into())
}
