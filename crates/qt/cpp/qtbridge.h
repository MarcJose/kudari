// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the front ends need of Qt itself, for `src/qt.rs`.
#pragma once

#include <QQuickItem>
#include <QString>

class QFont;
class QInputMethodEvent;
class QPainter;

/// Generic, so that cxx can bind it to the concrete item type.
template<typename Item>
void rk_enable_input_method(Item *item)
{
    item->setFlag(QQuickItem::ItemAcceptsInputMethod, true);
}

QString rk_input_commit_string(const QInputMethodEvent *event);
QString rk_input_preedit_string(const QInputMethodEvent *event);

void rk_clipboard_set(const QString &text, bool selection);
QString rk_clipboard_get(bool selection);
void rk_clipboard_set_rich(const QString &text, const QString &html);
bool rk_application_has_focus();
QString rk_system_fonts_json();
QString rk_fixed_fonts();
void rk_draw_badge(QPainter &painter,
                   double width,
                   const QString &text,
                   const QString &color,
                   const QString &family,
                   int size,
                   bool textOnly,
                   int transparency,
                   const QFont &terminalFont,
                   int cellHeight);
void rk_font_rendering(QFont &font, bool antiAlias, bool hinting);
void rk_font_family(QFont &font, const QString &family);
QString rk_color_from_text(const QString &text);
void rk_ask_save_file(int terminal, int purpose, const QString &suggestion);
void rk_print_text(const QString &text);
void rk_ask_history_lines(int terminal, int current, const QString &title, const QString &label);

QString rk_screen_names();
QString rk_pointer_screen();
QString rk_primary_screen();
QString rk_screen_available_size(const QString &name);
QString rk_palette_colors();
void rk_open_url_with_qt(const QString &url);
QString rk_ui_languages();
QString rk_key_sequence_text(int key, int modifiers);
QString rk_native_key_sequence(const QString &portable);
