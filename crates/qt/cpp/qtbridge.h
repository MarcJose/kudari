// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the front ends need of Qt itself, for `src/qt.rs`.
#pragma once

#include <QColor>
#include <QGlyphRun>
#include <QImage>
#include <QPainter>
#include <QPointF>
#include <QQuickItem>
#include <QRawFont>
#include <QString>

#include <cstdint>
#include <memory>

#include "rust/cxx.h"

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
/// Takes a fill while a frame is drawn on the graphics card, in a color as `0xAARRGGBB`, as the
/// colors of the others; false otherwise, and the painter has to fill it.
bool rk_collect_fill(const QRectF &rect, std::uint32_t argb);
/// Takes glyphs while a frame is drawn on the graphics card, as `rk_draw_glyphs` draws them;
/// false otherwise, and the painter has to draw them. `key` tells the font apart from the
/// other fonts the item draws with.
/// Takes a text while a frame is drawn on the graphics card, as `QPainter::drawText` draws it
/// from a point on the baseline; false otherwise.
bool rk_collect_text(std::uint64_t key, const QFont &font, const QString &text, const QPointF &position, std::uint32_t argb);
bool rk_collect_glyphs(std::uint64_t key,
                       const QRawFont &font,
                       std::uint32_t argb,
                       rust::Slice<const std::uint32_t> glyphs,
                       rust::Slice<const QPointF> positions);

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
    /// Whether the frame is drawn on the graphics card where Qt Quick draws through its RHI:
    /// its fills and glyphs as geometry, the glyphs from an atlas that holds each once. A
    /// frame with something that only the painter draws is painted into the image instead.
    virtual bool drawsOnGpu() const = 0;
    /// A frame for the graphics card had something that only the painter draws, or did not
    /// fit in the atlas, and was painted instead.
    virtual void gpuFallback() const = 0;
    /// Whether every frame covers all of the item: it is then neither cleared before nor
    /// blended with what lies behind it.
    void setOpaquePainting(bool opaque);
    /// Paints the frame and the cursor again.
    void updateContent();
    /// Paints only the cursor again, or only shows more or less of it.
    void updateCursor();
    /// Paints the lines that changed again, into the image as it is; see `paintLines`.
    void updateLines();
    /// What `paintLines` did: nothing, the lines that changed, nothing as the item has to paint
    /// the whole frame, or the whole frame itself.
    enum PaintedLines { NoLines = 0, SomeLines = 1, WholeFrame = 2, AllLines = 3 };
    /// Paints the lines that changed since the last frame into the image of the item, which
    /// holds that frame, each within a band of `rk_painter_band`; gives `WholeFrame` where it
    /// cannot tell them, and the item then paints all of it with `paint`.
    virtual int paintLines(QPainter *painter) = 0;

protected:
    QSGNode *updatePaintNode(QSGNode *node, UpdatePaintNodeData *data) override;
    void geometryChange(const QRectF &newGeometry, const QRectF &oldGeometry) override;
    void releaseResources() override;

private:
    QImage m_image;
    QImage m_cursor;
    bool m_opaque = false;
    bool m_contentDirty = true;
    bool m_linesDirty = false;
    /// Whether frames are drawn on the graphics card, and whether the last one was.
    bool m_gpu = false;
    bool m_shownOnGpu = false;
    /// The size in pixels of the device of the frame shown.
    QSize m_shownSize;
    /// What the painter of a frame on the graphics card paints on: the fonts take its density
    /// of pixels, and it holds nothing.
    QImage m_probe;
};
std::unique_ptr<QRawFont> rk_raw_font(const QPainter &painter, const QFont &font);
/// A text laid out once, for `rk_shape`.
struct RkShaped {
    QList<QGlyphRun> runs;
};
std::unique_ptr<RkShaped> rk_shape(const QPainter &painter, const QFont &font, const QString &text);
std::size_t rk_shaped_runs(const RkShaped &shaped);
std::unique_ptr<QRawFont> rk_shaped_font(const RkShaped &shaped, std::size_t run);
rust::Vec<std::uint32_t> rk_shaped_glyphs(const RkShaped &shaped, std::size_t run);
rust::Vec<double> rk_shaped_places(const RkShaped &shaped, std::size_t run);
bool rk_same_raw_font(const QRawFont &one, const QRawFont &other);
int rk_painter_dpi(const QPainter &painter);
std::uint32_t rk_glyph_index(const QRawFont &font, std::uint32_t character);
void rk_draw_glyphs(QPainter &painter,
                    const QRawFont &font,
                    const QColor &color,
                    rust::Slice<const std::uint32_t> glyphs,
                    rust::Slice<const QPointF> positions);
void rk_draw_text(QPainter &painter, const QPointF &position, const QString &text);
/// Lets the painter draw only within `band`, and clears that band to transparent first if
/// `clear`; `rk_painter_unclip` lets it draw everywhere again.
void rk_painter_band(QPainter &painter, const QRectF &band, bool clear);
void rk_painter_unclip(QPainter &painter);
void rk_font_family(QFont &font, const QString &family);
QString rk_color_from_text(const QString &text);
void rk_ask_save_file(int terminal, int purpose, const QString &suggestion);
void rk_print_text(const QString &text);
void rk_ask_history_lines(int terminal, int current, const QString &title, const QString &label);

/// For the GUI test: paints each terminal that shows anew, as a frame with its cursor over
/// it, to `<path>-<terminal>.png`, and saves what the window shows in its place to
/// `<path>-<terminal>-shown.png`. Nothing without the test hooks.
void rk_test_paint_terminals(const QString &path);
QString rk_screen_names();
QString rk_pointer_screen();
QString rk_primary_screen();
QString rk_screen_available_size(const QString &name);
QString rk_palette_colors();
void rk_open_url_with_qt(const QString &url);
QString rk_ui_languages();
void rk_release_when_hidden();
void rk_release_after(int milliseconds);
QString rk_key_sequence_text(int key, int modifiers);
int rk_base_key(int key, int modifiers, int scanCode, const QString &text);
QString rk_native_key_sequence(const QString &portable);
