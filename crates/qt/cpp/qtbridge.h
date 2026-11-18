// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the front ends need of Qt itself, for `src/qt.rs`.
#pragma once

#include <QImage>
#include <QPainter>
#include <QPointF>
#include <QQuickItem>
#include <QRawFont>
#include <QString>

#include <cstdint>
#include <memory>

#include "rust/cxx.h"

class QColor;
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
/// An item that paints into an image of its own and shows it as a texture: the base of the
/// terminal. Its image has the format that text draws fastest into and that most graphics
/// drivers take as it is, so no frame is converted or copied on its way to the screen, as
/// they are with QQuickPaintedItem. An item without a size keeps neither image nor texture.
///
/// The cursor has a small image of its own over that of the item: blinking and fading only
/// change how much of it shows, and a change of the cursor alone paints only its cells.
class RkImageItem : public QQuickItem
{
public:
    explicit RkImageItem(QQuickItem *parent = nullptr);

    /// Paints a frame; the painter draws into the image of the item, in logical pixels.
    virtual void paint(QPainter *painter) = 0;
    /// Paints the cursor, in the logical pixels of the item, into an image as large as
    /// `cursorRoom`, starting at its corner; gives the rectangle of the item that it covers,
    /// or an empty one without a cursor. `alone` tells that the frame was not painted with it.
    /// Where `mayKeep` allows it and the cursor looks as last time, it paints nothing and sets
    /// `kept`: the texture it has stays.
    virtual QRectF paintCursor(QPainter *painter, bool alone, bool mayKeep, bool &kept) = 0;
    /// The logical size that the cursor of any cell fits in.
    virtual QSizeF cursorRoom() const = 0;
    /// How much of the cursor shows, from 0 to 1.
    virtual double cursorOpacity() const = 0;
    /// Whether every frame covers all of the item: it is then neither cleared before nor
    /// blended with what lies behind it.
    void setOpaquePainting(bool opaque);
    /// Paints the frame and the cursor again.
    void updateContent();
    /// Paints only the cursor again, or only shows more or less of it.
    void updateCursor();

protected:
    QSGNode *updatePaintNode(QSGNode *node, UpdatePaintNodeData *data) override;
    void geometryChange(const QRectF &newGeometry, const QRectF &oldGeometry) override;

private:
    QImage m_image;
    QImage m_cursor;
    bool m_opaque = false;
    bool m_contentDirty = true;
};
std::unique_ptr<QRawFont> rk_raw_font(const QPainter &painter, const QFont &font);
std::uint32_t rk_glyph_index(const QRawFont &font, std::uint32_t character);
void rk_draw_glyphs(QPainter &painter,
                    const QRawFont &font,
                    const QColor &color,
                    rust::Slice<const std::uint32_t> glyphs,
                    rust::Slice<const QPointF> positions);
void rk_draw_text(QPainter &painter, const QPointF &position, const QString &text);
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
int rk_base_key(int key, int modifiers, int scanCode, const QString &text);
QString rk_native_key_sequence(const QString &portable);
