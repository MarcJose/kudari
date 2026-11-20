// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the front ends need of Qt itself: the clipboard, fonts, the badge, the input method
// and the dialogs for files, printing and the scrollback.
#include "qtbridge.h"
#include "kudari-qt/src/qt.cxx.h"

#include <QClipboard>
#include <cmath>
#include <cstring>
#include <functional>
#include <optional>
#include <vector>
#include <QCursor>
#include <QDesktopServices>
#include <QDir>
#include <QLocale>
#include <QPalette>
#include <QScreen>
#include <QUrl>
#include <QFileDialog>
#include <QFont>
#include <QFontDatabase>
#include <QFontMetricsF>
#include <QGlyphRun>
#include <QGuiApplication>
#include <QHash>
#include <QInputDialog>
#include <QInputMethodEvent>
#include <QKeySequence>
#include <QMatrix4x4>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMimeData>
#include <QPainter>
#include <QPrintDialog>
#include <QPrinter>
#include <QQuickWindow>
#include <QRegion>
#include <QSGGeometryNode>
#include <QSGImageNode>
#include <QSGMaterial>
#include <QSGMaterialShader>
#include <QSGTransformNode>
#include <QSGNode>
#include <QSGTexture>
#include <QTextDocument>
#include <QTextLayout>
#include <QTimer>
#include <QTransform>
#ifdef __GLIBC__
#include <malloc.h>
#endif
#include <QVarLengthArray>
// Private, as nothing public names the key of a key event on its first level.
#include <QtGui/private/qkeymapper_p.h>
// Private, as nothing public tells whether a font has glyphs of its own colors.
#include <QtGui/private/qfontengine_p.h>
#include <QtGui/private/qrawfont_p.h>
// With limited guarantees of compatibility, for a texture that takes only the parts of the
// frame that changed.
#include <rhi/qrhi.h>
#ifdef Q_OS_WIN
#include <windows.h>
#endif

void rk_clipboard_set(const QString &text, bool selection)
{
    QGuiApplication::clipboard()->setText(text, selection ? QClipboard::Selection : QClipboard::Clipboard);
}

QString rk_clipboard_get(bool selection)
{
    return QGuiApplication::clipboard()->text(selection ? QClipboard::Selection : QClipboard::Clipboard);
}

QString rk_input_commit_string(const QInputMethodEvent *event)
{
    return event->commitString();
}

QString rk_input_preedit_string(const QInputMethodEvent *event)
{
    return event->preeditString();
}

bool rk_application_has_focus()
{
    return QGuiApplication::focusWindow() != nullptr;
}

QString rk_system_fonts_json()
{
    const auto describe = [](QFontDatabase::SystemFont kind) {
        const QFont font = QFontDatabase::systemFont(kind);
        return QJsonObject{{QStringLiteral("family"), font.family()}, {QStringLiteral("pointSize"), font.pointSizeF()}};
    };
    return QString::fromUtf8(
        QJsonDocument(QJsonObject{{QStringLiteral("general"), describe(QFontDatabase::GeneralFont)}}).toJson(QJsonDocument::Compact));
}

QString rk_fixed_fonts()
{
    QJsonArray result;
    const auto families = QFontDatabase::families();
    for (const QString &family : families) {
        if (QFontDatabase::isFixedPitch(family) && !QFontDatabase::isPrivateFamily(family)) {
            result.append(family);
        }
    }
    return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
}

// A short text in large letters in the top right corner, behind the terminal's text. Without a
// font size of its own, it is two terminal lines high.
void rk_draw_badge(QPainter &painter,
                   double width,
                   const QString &text,
                   const QString &color,
                   const QString &family,
                   int size,
                   bool textOnly,
                   int transparency,
                   const QFont &terminalFont,
                   int cellHeight)
{
    QColor ink(color);
    if (!ink.isValid()) {
        ink = QColor(Qt::gray);
    }
    ink.setAlpha(qBound(0, transparency, 255));

    QFont font = family.isEmpty() ? terminalFont : QFont(family);
    if (size > 0) {
        font.setPointSize(size);
    } else {
        font.setPixelSize(qMax(1, 2 * cellHeight));
    }

    const qreal margin = cellHeight / 2.0;
    const QSizeF needed = QFontMetricsF(font).boundingRect(QRectF(), Qt::AlignRight | Qt::AlignTop, text).size();
    const QRectF place(width - margin - needed.width(), margin, needed.width(), needed.height());

    painter.save();
    if (!textOnly) {
        QColor back = ink;
        back.setAlpha(ink.alpha() / 4);
        painter.setPen(Qt::NoPen);
        painter.setBrush(back);
        painter.drawRoundedRect(place.adjusted(-margin / 2, -margin / 4, margin / 2, margin / 4), margin / 2, margin / 2);
    }
    painter.setFont(font);
    painter.setPen(ink);
    painter.drawText(place, Qt::AlignRight | Qt::AlignTop, text);
    painter.restore();
}

void rk_font_rendering(QFont &font, bool antiAlias, bool hinting)
{
    if (!antiAlias) {
        font.setStyleStrategy(QFont::NoAntialias);
    }
    if (hinting) {
        font.setHintingPreference(QFont::PreferFullHinting);
    }
}

// The font as the painter would draw with it: `QPainter` resolves a font against its device.
std::unique_ptr<QRawFont> rk_raw_font(const QPainter &painter, const QFont &font)
{
    return std::make_unique<QRawFont>(QRawFont::fromFont(QFont(font, painter.device())));
}

std::unique_ptr<RkShaped> rk_shape(const QPainter &painter, const QFont &font, const QString &text)
{
    auto shaped = std::make_unique<RkShaped>();
    // As `drawText` lays a text out: in the font made for the device, on one line that does not
    // wrap, with the fonts that the font database falls back on for what the font lacks.
    QTextLayout layout(text, QFont(font, painter.device()), painter.device());
    QTextOption option;
    option.setWrapMode(QTextOption::NoWrap);
    layout.setTextOption(option);
    layout.beginLayout();
    QTextLine line = layout.createLine();
    if (!line.isValid()) {
        layout.endLayout();
        return shaped;
    }
    line.setLineWidth(1e6);
    layout.endLayout();
    // From the point on the baseline where the text starts, as `drawText` takes it.
    const QPointF baseline(0, line.ascent());
    for (QGlyphRun run : line.glyphRuns()) {
        QList<QPointF> positions = run.positions();
        for (QPointF &position : positions) {
            position -= baseline;
        }
        run.setPositions(positions);
        shaped->runs.append(run);
    }
    return shaped;
}

std::size_t rk_shaped_runs(const RkShaped &shaped)
{
    return std::size_t(shaped.runs.size());
}

std::unique_ptr<QRawFont> rk_shaped_font(const RkShaped &shaped, std::size_t run)
{
    return std::make_unique<QRawFont>(shaped.runs.at(qsizetype(run)).rawFont());
}

rust::Vec<std::uint32_t> rk_shaped_glyphs(const RkShaped &shaped, std::size_t run)
{
    rust::Vec<std::uint32_t> glyphs;
    for (quint32 glyph : shaped.runs.at(qsizetype(run)).glyphIndexes()) {
        glyphs.push_back(glyph);
    }
    return glyphs;
}

rust::Vec<double> rk_shaped_places(const RkShaped &shaped, std::size_t run)
{
    rust::Vec<double> places;
    for (const QPointF &position : shaped.runs.at(qsizetype(run)).positions()) {
        places.push_back(position.x());
        places.push_back(position.y());
    }
    return places;
}

bool rk_same_raw_font(const QRawFont &one, const QRawFont &other)
{
    return one == other;
}

int rk_painter_dpi(const QPainter &painter)
{
    return painter.device() ? painter.device()->logicalDpiY() : 0;
}

// 0 when the font lacks the character, which then needs the fallback of `QPainter::drawText`.
std::uint32_t rk_glyph_index(const QRawFont &font, std::uint32_t character)
{
    const char32_t ucs4 = character;
    const QString text = QString::fromUcs4(&ucs4, 1);
    quint32 glyphs[2] = {0, 0};
    int count = 2;
    if (!font.glyphIndexesForChars(text.constData(), int(text.size()), glyphs, &count) || count != 1) {
        return 0;
    }
    return glyphs[0];
}

// Each item paints into an image of its own in ARGB32_Premultiplied, which the raster engine of
// Qt draws text into several times faster than into RGBA8888, and which most graphics drivers
// take as it is; see `updatePaintNode`.
RkImageItem::RkImageItem(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
}

void RkImageItem::setOpaquePainting(bool opaque)
{
    m_opaque = opaque;
}

void RkImageItem::updateContent()
{
    m_contentDirty = true;
    update();
}

void RkImageItem::updateCursor()
{
    update();
}

void RkImageItem::updateLines()
{
    m_linesDirty = true;
    update();
}

// The bands that a frame of lines paints, in pixels of the device, while
// `RkImageItem::updatePaintNode` collects them on its thread.
static thread_local QRegion *t_bands = nullptr;
void rk_painter_band(QPainter &painter, const QRectF &band, bool clear)
{
    if (t_bands) {
        *t_bands += painter.deviceTransform().mapRect(band).toAlignedRect();
    }
    painter.setClipRect(band);
    if (clear) {
        painter.save();
        painter.setCompositionMode(QPainter::CompositionMode_Source);
        painter.fillRect(band, Qt::transparent);
        painter.restore();
    }
}

void rk_painter_unclip(QPainter &painter)
{
    painter.setClipping(false);
}

// The scene graph went, as for a window hidden for long: the images go with it, and the next
// frame paints all of it anew.
void RkImageItem::releaseResources()
{
    m_image = QImage();
    m_cursor = QImage();
    m_contentDirty = true;
    QQuickItem::releaseResources();
}

void RkImageItem::geometryChange(const QRectF &newGeometry, const QRectF &oldGeometry)
{
    QQuickItem::geometryChange(newGeometry, oldGeometry);
    if (newGeometry.size() != oldGeometry.size()) {
        updateContent();
    } else if (newGeometry.topLeft() != oldGeometry.topLeft()) {
        // A new place can lie differently between the pixels of the device.
        update();
    }
}

// The texture of a frame where Qt Quick draws through the RHI: it keeps its storage on the
// graphics card from frame to frame and takes only the parts of the image that changed, where
// a new texture would take all of it each time. The image is held only until the render
// thread takes it, before the next frame paints into it again.
class RkFrameTexture : public QSGTexture
{
public:
    ~RkFrameTexture() override { delete m_texture; }

    qint64 comparisonKey() const override { return qint64(quintptr(this)); }
    QRhiTexture *rhiTexture() const override { return m_texture; }
    QSize textureSize() const override { return m_size; }
    bool hasAlphaChannel() const override { return m_alpha; }
    bool hasMipmaps() const override { return false; }

    // The image to show; only `parts` of it changed since the last, in pixels of the device,
    // or all of it if `whole`.
    void take(const QImage &image, bool alpha, const QRegion &parts, bool whole)
    {
        m_whole = m_whole || whole || image.size() != m_size || alpha != m_alpha;
        m_parts += parts;
        m_image = image;
        m_size = image.size();
        m_alpha = alpha;
    }

    void commitTextureOperations(QRhi *rhi, QRhiResourceUpdateBatch *batch) override
    {
        if (m_image.isNull()) {
            return;
        }
        // As Qt's own textures take an image: as it is where the device can, else converted.
        const bool bgra = rhi->isTextureFormatSupported(QRhiTexture::BGRA8);
        const QRhiTexture::Format format = bgra ? QRhiTexture::BGRA8 : QRhiTexture::RGBA8;
        if (!m_texture || m_texture->pixelSize() != m_size || m_texture->format() != format) {
            delete m_texture;
            m_texture = rhi->newTexture(format, m_size);
            if (!m_texture->create()) {
                delete m_texture;
                m_texture = nullptr;
                return;
            }
            m_whole = true;
        }
        const QRegion parts = m_whole ? QRegion(QRect(QPoint(), m_size)) : (m_parts & QRect(QPoint(), m_size));
        QVarLengthArray<QRhiTextureUploadEntry, 8> entries;
        for (const QRect &part : parts) {
            QRhiTextureSubresourceUploadDescription description;
            if (bgra) {
                description = QRhiTextureSubresourceUploadDescription(m_image);
                description.setSourceTopLeft(part.topLeft());
                description.setSourceSize(part.size());
            } else {
                description = QRhiTextureSubresourceUploadDescription(
                    m_image.copy(part).convertToFormat(QImage::Format_RGBA8888_Premultiplied));
            }
            description.setDestinationTopLeft(part.topLeft());
            entries.append(QRhiTextureUploadEntry(0, 0, description));
        }
        if (!entries.isEmpty()) {
            QRhiTextureUploadDescription upload;
            upload.setEntries(entries.cbegin(), entries.cend());
            batch->uploadTexture(m_texture, upload);
        }
        m_image = QImage();
        m_parts = QRegion();
        m_whole = false;
    }

private:
    QRhiTexture *m_texture = nullptr;
    QImage m_image;
    QSize m_size;
    QRegion m_parts;
    bool m_alpha = true;
    bool m_whole = true;
};

// Gives `node` the image as a new texture, deleting the one it had once, whether or not the
// node does so itself when another takes its place.
static void showImage(QQuickWindow *window, QSGImageNode *node, const QImage &image, bool opaque, const QRectF &rect, const QSize &size, bool smooth)
{
    QSGTexture *texture =
        window->createTextureFromImage(image, opaque ? QQuickWindow::TextureIsOpaque : QQuickWindow::TextureHasAlphaChannel);
    QSGTexture *previous = node->texture();
    node->setOwnsTexture(false);
    node->setTexture(texture);
    node->setOwnsTexture(true);
    delete previous;
    node->setRect(rect);
    node->setSourceRect(QRectF(QPointF(), QSizeF(size)));
    node->setFiltering(smooth ? QSGTexture::Linear : QSGTexture::Nearest);
}

// Shows the frame in `node`: through the RHI in the texture it keeps, with only `parts` of
// the image new unless `whole`; in software as a new texture.
static void showFrame(QQuickWindow *window, QSGImageNode *node, const QImage &image, bool opaque, const QRectF &rect, const QSize &size, bool smooth, const QRegion &parts, bool whole)
{
    if (!window->rhi()) {
        showImage(window, node, image, opaque, rect, size, smooth);
        return;
    }
    auto texture = dynamic_cast<RkFrameTexture *>(node->texture());
    if (!texture) {
        texture = new RkFrameTexture;
        QSGTexture *previous = node->texture();
        node->setOwnsTexture(false);
        node->setTexture(texture);
        node->setOwnsTexture(true);
        delete previous;
        whole = true;
    }
    texture->take(image, !opaque, parts, whole);
    node->setRect(rect);
    node->setSourceRect(QRectF(QPointF(), QSizeF(size)));
    node->setFiltering(smooth ? QSGTexture::Linear : QSGTexture::Nearest);
    // The same texture with new content: the frame has to be drawn again.
    node->markDirty(QSGNode::DirtyMaterial);
}

#ifdef RK_TEST_HOOKS
// The items of `item` and below that are images of terminals.
static void imageItems(QQuickItem *item, QList<RkImageItem *> &found)
{
    if (auto image = dynamic_cast<RkImageItem *>(item)) {
        found.append(image);
    }
    for (QQuickItem *child : item->childItems()) {
        imageItems(child, found);
    }
}

void rk_test_paint_terminals(const QString &path)
{
    for (QWindow *window : QGuiApplication::topLevelWindows()) {
        auto quick = qobject_cast<QQuickWindow *>(window);
        if (!quick || !quick->isVisible()) {
            continue;
        }
        QList<RkImageItem *> items;
        imageItems(quick->contentItem(), items);
        // What the window shows, from its scene graph, before anything paints anew.
        const QImage shown = quick->grabWindow();
        const qreal ratio = quick->effectiveDevicePixelRatio();
        for (RkImageItem *item : items) {
            const QSize size = (QSizeF(item->width(), item->height()) * ratio).toSize();
            if (!item->isVisible() || size.isEmpty()) {
                continue;
            }
            const QString name = QStringLiteral("%1-%2").arg(path).arg(item->property("terminalId").toInt());
            QImage frame(size, QImage::Format_ARGB32_Premultiplied);
            frame.setDevicePixelRatio(ratio);
            frame.fill(Qt::transparent);
            QPainter painter(&frame);
            item->paint(&painter);
            const QSize room = (item->cursorRoom() * ratio).toSize().expandedTo(QSize(1, 1));
            QImage cursor(room, QImage::Format_ARGB32_Premultiplied);
            cursor.setDevicePixelRatio(ratio);
            cursor.fill(Qt::transparent);
            QRectF where;
            {
                QPainter cursorPainter(&cursor);
                bool kept = false;
                where = item->paintCursor(&cursorPainter, false, false, kept);
            }
            if (!where.isEmpty()) {
                painter.setOpacity(item->cursorOpacity());
                const QSize covered = (where.size() * ratio).toSize().boundedTo(room);
                painter.drawImage(where.topLeft(), cursor, QRectF(QPointF(), QSizeF(covered)));
            }
            painter.end();
            frame.save(name + QStringLiteral(".png"));
            const QPointF corner = item->mapToScene(QPointF());
            const QRect place(QPoint(qRound(corner.x() * ratio), qRound(corner.y() * ratio)), size);
            shown.copy(place).save(name + QStringLiteral("-shown.png"));
        }
    }
}
#else
void rk_test_paint_terminals(const QString &)
{
}
#endif

// A frame on the graphics card: its fills and glyphs, in the order the frame paints them, as
// two triangles each in one geometry with one material. A glyph takes its pixels from an
// atlas that holds it once; a fill takes a white pixel of it.

// A corner of a fill or glyph, in the logical pixels of the item. Its place in the atlas is in
// pixels of the atlas while the frame is collected, then from 0 to 1.
struct RkVertex {
    float x;
    float y;
    float u;
    float v;
    // The color, with its alpha multiplied in.
    uchar red;
    uchar green;
    uchar blue;
    uchar alpha;
    // 255 in the first where the colors come from the atlas, as for an emoji.
    uchar kind[4];
};

static const QSGGeometry::AttributeSet &vertexAttributes()
{
    static const QSGGeometry::Attribute attributes[] = {
        QSGGeometry::Attribute::createWithAttributeType(0, 2, QSGGeometry::FloatType, QSGGeometry::PositionAttribute),
        QSGGeometry::Attribute::createWithAttributeType(1, 2, QSGGeometry::FloatType, QSGGeometry::TexCoordAttribute),
        QSGGeometry::Attribute::createWithAttributeType(2, 4, QSGGeometry::UnsignedByteType, QSGGeometry::ColorAttribute),
        QSGGeometry::Attribute::createWithAttributeType(3, 4, QSGGeometry::UnsignedByteType, QSGGeometry::UnknownAttribute),
    };
    static const QSGGeometry::AttributeSet set = {4, int(sizeof(RkVertex)), attributes};
    return set;
}

// The glyphs of an item, each drawn once by `QPainter`, white or in its own colors, at its place
// within a pixel of the device: wherever a frame shows it on that place, it covers the same
// pixels as in a painted frame. Tiles go in rows by height; the atlas grows twice as large where
// they do not fit, up to the largest texture of the device, and starts again when that is full.
class RkGlyphAtlas
{
public:
    struct Tile {
        // In the atlas; empty for a glyph without pixels, as a space.
        QRect rect;
        // From the pixel of the device that holds the point of the glyph to the corner of the tile.
        QPoint offset;
        bool ownColors = false;
    };

    RkGlyphAtlas() { reset(1.0); }

    // For a frame at `ratio` pixels of the device for each logical one, on a device whose
    // textures can be `most` pixels wide.
    void begin(qreal ratio, int most)
    {
        m_most = qMin(most, Most);
        if (ratio != m_ratio || m_full) {
            reset(ratio);
        }
    }

    // The tile of `glyph` of `font` at `place`, the logical point where `rk_draw_glyphs` puts it,
    // which lies in the pixel `pixel` of the device; nothing where the atlas is full: it starts
    // empty with the next frame.
    std::optional<Tile> tile(std::uint64_t key, const QRawFont &font, quint32 glyph, const QPointF &place, const QPoint &pixel)
    {
        const std::pair<std::uint64_t, quint64> name(key, (quint64(glyph) << 26) | steps(place, pixel));
        const auto found = m_tiles.constFind(name);
        if (found != m_tiles.constEnd()) {
            return *found;
        }
        // Room for what reaches out of the box of the glyph, as antialiasing and the bitmaps of
        // emoji can.
        const QFontEngine *engine = QRawFontPrivate::get(font)->fontEngine;
        const std::optional<Tile> made = make(font.boundingRect(glyph), font.pixelSize(), pixel, engine && engine->isColorFont(), [&](QPainter &painter) {
            QGlyphRun run;
            run.setRawFont(font);
            run.setRawData(&glyph, &place, 1);
            painter.drawGlyphRun(QPointF(), run);
        });
        if (made) {
            m_tiles.insert(name, *made);
        }
        return made;
    }

    // The tile of `text` in `font`, as `QPainter::drawText` draws it from `place` on: a text
    // whose glyphs no font has, or that `rk_shape` cannot tell. A tile with pixels of other
    // colors than white keeps them, as an emoji does.
    std::optional<Tile> textTile(std::uint64_t key, const QFont &font, const QString &text, const QPointF &place, const QPoint &pixel)
    {
        const QString name = QString::number(key) + QLatin1Char('/') + QString::number(steps(place, pixel)) + QLatin1Char('/') + text;
        const auto found = m_texts.constFind(name);
        if (found != m_texts.constEnd()) {
            return *found;
        }
        // A program can show texts without end, also such that draw nothing and so take no
        // room: their tiles are forgotten from some number on. Those in the atlas fill it, and
        // it starts empty when full.
        if (m_texts.size() >= MostTexts) {
            m_texts.clear();
        }
        // The metrics on an image, as the painter of a frame resolves its font.
        static const QImage device(1, 1, QImage::Format_ARGB32_Premultiplied);
        const QFont resolved(font, &device);
        const QFontMetricsF metrics(resolved, &device);
        const QRectF box = metrics.boundingRect(text).united(QRectF(0, -metrics.ascent(), metrics.horizontalAdvance(text), metrics.height()));
        const std::optional<Tile> made = make(box, metrics.height(), pixel, false, [&](QPainter &painter) {
            painter.setFont(font);
            painter.drawText(place, text);
        });
        if (made) {
            m_texts.insert(name, *made);
        }
        return made;
    }

    // The middle of a white block, for fills.
    static QPointF white() { return {1, 1}; }
    QSize size() const { return m_image.size(); }
    // The texture, with what changed since the last frame.
    QSGTexture *texture()
    {
        if (m_whole || !m_fresh.isEmpty()) {
            m_texture->take(m_image, true, m_fresh, m_whole);
            m_fresh = QRect();
            m_whole = false;
        }
        return m_texture.get();
    }

private:
    static constexpr int Start = 512;
    static constexpr int Most = 4096;
    static constexpr qsizetype MostTexts = 16384;

    void reset(qreal ratio)
    {
        m_ratio = ratio;
        m_full = false;
        m_image = QImage(Start, Start, QImage::Format_ARGB32_Premultiplied);
        m_image.fill(Qt::transparent);
        // A white block for fills, in the first row.
        for (int y = 0; y < 2; ++y) {
            auto line = reinterpret_cast<quint32 *>(m_image.scanLine(y));
            line[0] = line[1] = 0xffffffff;
        }
        m_rows = {{0, 2, 3}};
        m_bottom = 3;
        m_tiles.clear();
        m_texts.clear();
        m_fresh = QRect();
        m_whole = true;
    }

    // The part of a pixel that `place` lies at, in steps that tell apart those of `QPainter`.
    quint64 steps(const QPointF &place, const QPoint &pixel) const
    {
        const QPointF part = place * m_ratio - QPointF(pixel);
        const auto step = [](qreal value) { return quint64(qBound(0, qRound(value * 4096), 4096)); };
        return (step(part.x()) << 13) | step(part.y());
    }

    // A tile of what `draw` draws in white, at the same scale as the painter of a frame and
    // moved by whole pixels, so that it lands on the same part of a pixel as there. `box` is
    // where it draws around its point, in logical pixels, `size` the size of its font.
    template<typename Draw>
    std::optional<Tile> make(const QRectF &box, qreal size, const QPoint &pixel, bool ownColors, Draw draw)
    {
        const qreal margin = 2 + std::ceil(size * m_ratio / 8);
        const QRect room = QRectF(box.left() * m_ratio - margin,
                                  box.top() * m_ratio - margin,
                                  box.width() * m_ratio + 2 * margin,
                                  box.height() * m_ratio + 2 * margin)
                               .toAlignedRect()
                               .intersected(QRect(-Most / 2, -Most / 2, Most, Most));
        Tile tile;
        tile.ownColors = ownColors;
        if (room.isEmpty()) {
            return tile;
        }
        QImage drawn(room.size(), QImage::Format_ARGB32_Premultiplied);
        drawn.fill(Qt::transparent);
        {
            QPainter painter(&drawn);
            painter.setTransform(QTransform(m_ratio, 0, 0, m_ratio, -qreal(pixel.x() + room.left()), -qreal(pixel.y() + room.top())));
            painter.setPen(Qt::white);
            draw(painter);
        }
        bool colored = false;
        const QRect used = usedPart(drawn, colored);
        if (used.isEmpty()) {
            return tile;
        }
        tile.ownColors = ownColors || colored;
        QPoint where;
        if (!placeTile(used.size(), where)) {
            m_full = true;
            return std::nullopt;
        }
        for (int y = 0; y < used.height(); ++y) {
            std::memcpy(m_image.scanLine(where.y() + y) + where.x() * 4,
                        drawn.constScanLine(used.top() + y) + used.left() * 4,
                        std::size_t(used.width()) * 4);
        }
        tile.rect = QRect(where, used.size());
        tile.offset = room.topLeft() + used.topLeft();
        m_fresh = m_fresh.united(tile.rect);
        return tile;
    }

    // The smallest rectangle that holds every pixel the painter drew, and whether any of them
    // has a color of its own rather than white.
    static QRect usedPart(const QImage &image, bool &colored)
    {
        int left = image.width(), right = -1, top = image.height(), bottom = -1;
        for (int y = 0; y < image.height(); ++y) {
            const auto line = reinterpret_cast<const quint32 *>(image.constScanLine(y));
            for (int x = 0; x < image.width(); ++x) {
                const quint32 pixel = line[x];
                if (pixel != 0) {
                    left = qMin(left, x);
                    right = qMax(right, x);
                    top = qMin(top, y);
                    bottom = y;
                    colored = colored || qRed(pixel) != qGreen(pixel) || qGreen(pixel) != qBlue(pixel);
                }
            }
        }
        return right < 0 ? QRect() : QRect(QPoint(left, top), QPoint(right, bottom));
    }

    // A place for a tile of `size`, with a pixel between tiles; grows the atlas where needed.
    bool placeTile(const QSize &size, QPoint &where)
    {
        for (;;) {
            for (Row &row : m_rows) {
                // A row of a height that the tile does not waste much of.
                if (size.height() <= row.height && size.height() * 5 >= row.height * 4 && row.x + size.width() <= m_image.width()) {
                    where = QPoint(row.x, row.y);
                    row.x += size.width() + 1;
                    return true;
                }
            }
            if (size.width() <= m_image.width() && m_bottom + size.height() <= m_image.height()) {
                m_rows.append({m_bottom, size.height(), size.width() + 1});
                where = QPoint(0, m_bottom);
                m_bottom += size.height() + 1;
                return true;
            }
            if (m_image.width() * 2 > m_most) {
                return false;
            }
            QImage larger(m_image.size() * 2, QImage::Format_ARGB32_Premultiplied);
            larger.fill(Qt::transparent);
            for (int y = 0; y < m_image.height(); ++y) {
                std::memcpy(larger.scanLine(y), m_image.constScanLine(y), std::size_t(m_image.bytesPerLine()));
            }
            m_image = larger;
            m_whole = true;
        }
    }

    struct Row {
        int y;
        int height;
        // Where the next tile of the row goes.
        int x;
    };

    std::unique_ptr<RkFrameTexture> m_texture = std::make_unique<RkFrameTexture>();
    QImage m_image;
    QList<Row> m_rows;
    int m_bottom = 0;
    // By the key of the font, the glyph and its part of a pixel.
    QHash<std::pair<std::uint64_t, quint64>, Tile> m_tiles;
    // By the key of the font, the part of a pixel and the text.
    QHash<QString, Tile> m_texts;
    QRect m_fresh;
    bool m_whole = true;
    bool m_full = false;
    qreal m_ratio = 1.0;
    int m_most = Most;
};

class RkGlyphShader : public QSGMaterialShader
{
public:
    RkGlyphShader()
    {
        setShaderFileName(VertexStage, QStringLiteral(":/kudari/shaders/glyph.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(":/kudari/shaders/glyph.frag.qsb"));
    }

    bool updateUniformData(RenderState &state, QSGMaterial *, QSGMaterial *) override
    {
        QByteArray *data = state.uniformData();
        bool changed = false;
        if (state.isMatrixDirty()) {
            const QMatrix4x4 matrix = state.combinedMatrix();
            std::memcpy(data->data(), matrix.constData(), 64);
            changed = true;
        }
        if (state.isOpacityDirty()) {
            const float opacity = state.opacity();
            std::memcpy(data->data() + 64, &opacity, 4);
            changed = true;
        }
        return changed;
    }

    void updateSampledImage(RenderState &state, int binding, QSGTexture **texture, QSGMaterial *material, QSGMaterial *) override;
};

class RkGlyphMaterial : public QSGMaterial
{
public:
    // Drawn as it is, not merged with other nodes: Qt would copy and move each corner on the
    // processor first.
    RkGlyphMaterial() { setFlag(Blending | RequiresFullMatrix); }

    QSGMaterialType *type() const override
    {
        static QSGMaterialType type;
        return &type;
    }
    QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override { return new RkGlyphShader; }
    int compare(const QSGMaterial *other) const override
    {
        return this == other ? 0 : (std::less<const QSGMaterial *>()(this, other) ? -1 : 1);
    }

    RkGlyphAtlas atlas;
    // How many corners of the geometry the last frame wrote.
    std::size_t shown = 0;
};

void RkGlyphShader::updateSampledImage(RenderState &state, int binding, QSGTexture **texture, QSGMaterial *material, QSGMaterial *)
{
    if (binding != 1) {
        return;
    }
    QSGTexture *atlas = static_cast<RkGlyphMaterial *>(material)->atlas.texture();
    atlas->setFiltering(QSGTexture::Nearest);
    atlas->commitTextureOperations(state.rhi(), state.resourceUpdateBatch());
    *texture = atlas;
}

// What a frame on the graphics card collects while it paints on the render thread: the
// corners of its fills and glyphs, straight into the geometry of the node. The geometry grows
// and shrinks in large steps, so that it keeps its memory from frame to frame; corners beyond
// the frame are all zero, and their triangles cover nothing.
struct RkGpuFrame {
    RkGlyphAtlas *atlas;
    qreal ratio;
    QSGGeometry *geometry;
    // The corners the frame wrote, and up to where those of the geometry may not be zero.
    std::size_t used = 0;
    std::size_t written = 0;
    // The size of the atlas that the places in it are a part of.
    QSize atlasSize;
    // Something of the frame did not fit in the atlas: it is painted.
    bool failed = false;
};
static thread_local RkGpuFrame *t_gpu = nullptr;

// A color as the vertices take it: with its alpha multiplied in, and whether the colors come
// from the atlas.
struct RkVertexColor {
    uchar red;
    uchar green;
    uchar blue;
    uchar alpha;
    uchar ownColors;
};

static RkVertexColor vertexColor(QRgb rgba, bool ownColors)
{
    const int alpha = qAlpha(rgba);
    const auto channel = [alpha](int value) { return uchar(value * alpha / 255); };
    return {channel(qRed(rgba)), channel(qGreen(rgba)), channel(qBlue(rgba)), uchar(alpha), uchar(ownColors ? 255 : 0)};
}

// Two triangles over `place`, in logical pixels, with `source` of the atlas, in its pixels.
// The steps in which the geometry grows and shrinks, in corners. The GUI test takes small
// ones, and each frame starts with the least room, so that it grows it again and again and
// every pixel check sees that.
#ifdef RK_TEST_HOOKS
static constexpr std::size_t GeometryStep = 6 * 64;
#else
static constexpr std::size_t GeometryStep = 6 * 8192;
#endif

// The geometry with room for `count` corners, those of the frame so far kept.
static void makeRoom(RkGpuFrame &frame, std::size_t count)
{
    const std::vector<RkVertex> kept(static_cast<const RkVertex *>(frame.geometry->vertexData()),
                                     static_cast<const RkVertex *>(frame.geometry->vertexData()) + frame.used);
    const std::size_t room = (count / GeometryStep + 1) * GeometryStep;
    frame.geometry->allocate(int(room));
    std::memcpy(frame.geometry->vertexData(), kept.data(), kept.size() * sizeof(RkVertex));
    // New memory holds anything.
    frame.written = room;
}

static void addQuad(RkGpuFrame &frame, const QRectF &place, const QRectF &source, RkVertexColor color)
{
    if (frame.used + 6 > std::size_t(frame.geometry->vertexCount())) {
        makeRoom(frame, frame.used + 6);
    }
    // The atlas is a power of two wide and high: these are exact.
    const float across = 1.0f / float(frame.atlasSize.width());
    const float down = 1.0f / float(frame.atlasSize.height());
    const float left = float(place.left());
    const float top = float(place.top());
    const float right = float(place.left() + place.width());
    const float bottom = float(place.top() + place.height());
    const float sourceLeft = float(source.left()) * across;
    const float sourceTop = float(source.top()) * down;
    const float sourceRight = float(source.left() + source.width()) * across;
    const float sourceBottom = float(source.top() + source.height()) * down;
    RkVertex *vertex = static_cast<RkVertex *>(frame.geometry->vertexData()) + frame.used;
    const auto set = [&color](RkVertex &corner, float x, float y, float u, float v) {
        corner = {x, y, u, v, color.red, color.green, color.blue, color.alpha, {color.ownColors, 0, 0, 0}};
    };
    set(vertex[0], left, top, sourceLeft, sourceTop);
    set(vertex[1], right, top, sourceRight, sourceTop);
    set(vertex[2], left, bottom, sourceLeft, sourceBottom);
    vertex[3] = vertex[1];
    set(vertex[4], right, bottom, sourceRight, sourceBottom);
    vertex[5] = vertex[2];
    frame.used += 6;
}

// The places of the frame so far in an atlas that grew while it was collected.
static void followAtlas(RkGpuFrame &frame)
{
    const QSize size = frame.atlas->size();
    if (size == frame.atlasSize) {
        return;
    }
    const float across = float(frame.atlasSize.width()) / float(size.width());
    const float down = float(frame.atlasSize.height()) / float(size.height());
    auto vertices = static_cast<RkVertex *>(frame.geometry->vertexData());
    for (std::size_t i = 0; i < frame.used; ++i) {
        RkVertex &corner = vertices[i];
        corner.u *= across;
        corner.v *= down;
    }
    frame.atlasSize = size;
}

// Shows the tile of a glyph or text whose point lies in `pixel`.
static void addTile(RkGpuFrame &frame, const RkGlyphAtlas::Tile &tile, const QPoint &pixel, RkVertexColor color)
{
    if (tile.rect.isEmpty()) {
        return;
    }
    followAtlas(frame);
    const QPoint corner = pixel + tile.offset;
    const QRectF shown(QPointF(corner) / frame.ratio, QSizeF(tile.rect.size()) / frame.ratio);
    color.ownColors = tile.ownColors ? 255 : 0;
    addQuad(frame, shown, QRectF(tile.rect), color);
}

bool rk_collect_fill(const QRectF &rect, std::uint32_t argb)
{
    if (!t_gpu) {
        return false;
    }
    addQuad(*t_gpu, rect, QRectF(RkGlyphAtlas::white(), QSizeF()), vertexColor(argb, false));
    return true;
}

bool rk_collect_glyphs(std::uint64_t key,
                       const QRawFont &font,
                       std::uint32_t argb,
                       rust::Slice<const std::uint32_t> glyphs,
                       rust::Slice<const QPointF> positions)
{
    if (!t_gpu) {
        return false;
    }
    RkGpuFrame &frame = *t_gpu;
    const RkVertexColor packed = vertexColor(argb, false);
    const std::size_t count = qMin(glyphs.size(), positions.size());
    for (std::size_t i = 0; i < count && !frame.failed; ++i) {
        // As `rk_draw_glyphs` places it, and the pixel of the device it lands in.
        const QPointF place(std::floor(positions[i].x() * 64) / 64, std::floor(positions[i].y() * 64) / 64);
        const QPoint pixel(int(std::floor(place.x() * frame.ratio)), int(std::floor(place.y() * frame.ratio)));
        const std::optional<RkGlyphAtlas::Tile> tile = frame.atlas->tile(key, font, glyphs[i], place, pixel);
        if (!tile) {
            frame.failed = true;
        } else {
            addTile(frame, *tile, pixel, packed);
        }
    }
    return true;
}

bool rk_collect_text(std::uint64_t key, const QFont &font, const QString &text, const QPointF &position, std::uint32_t argb)
{
    if (!t_gpu) {
        return false;
    }
    RkGpuFrame &frame = *t_gpu;
    if (frame.failed) {
        return true;
    }
    const QPoint pixel(int(std::floor(position.x() * frame.ratio)), int(std::floor(position.y() * frame.ratio)));
    const std::optional<RkGlyphAtlas::Tile> tile = frame.atlas->textTile(key, font, text, position, pixel);
    if (!tile) {
        frame.failed = true;
    } else {
        addTile(frame, *tile, pixel, vertexColor(argb, false));
    }
    return true;
}

// Ends the geometry of the frame in `node`: no corners of the frame before beyond those of
// this one, and less room where it holds far fewer.
static void showGpuFrame(QSGGeometryNode *node, RkGpuFrame &frame)
{
    followAtlas(frame);
    if (frame.used + 2 * GeometryStep < std::size_t(frame.geometry->vertexCount())) {
        makeRoom(frame, frame.used);
    }
    if (frame.written > frame.used) {
        auto vertices = static_cast<RkVertex *>(frame.geometry->vertexData());
        std::memset(static_cast<void *>(vertices + frame.used), 0, (frame.written - frame.used) * sizeof(RkVertex));
    }
    node->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
}

QSGNode *RkImageItem::updatePaintNode(QSGNode *node, UpdatePaintNodeData *)
{
    QQuickWindow *shown = window();
    const qreal ratio = shown ? shown->effectiveDevicePixelRatio() : 1.0;
    const QSize size = (QSizeF(width(), height()) * ratio).toSize();
    if (!shown || size.isEmpty()) {
        // A terminal in a tab in the background has no size: neither its pixels nor its
        // textures stay.
        m_image = QImage();
        m_cursor = QImage();
        delete node;
        m_contentDirty = true;
        m_shownOnGpu = false;
        return nullptr;
    }
    // A frame on the graphics card, or the image of a painted one; then the cursor over it,
    // which fades through an opacity of its own.
    QSGTransformNode *move = nullptr;
    QSGGeometryNode *drawn = nullptr;
    QSGImageNode *content = nullptr;
    QSGOpacityNode *fade = nullptr;
    QSGImageNode *cursor = nullptr;
    if (node) {
        move = static_cast<QSGTransformNode *>(node->firstChild());
        drawn = static_cast<QSGGeometryNode *>(move->firstChild());
        content = static_cast<QSGImageNode *>(move->nextSibling());
        fade = static_cast<QSGOpacityNode *>(content->nextSibling());
        cursor = static_cast<QSGImageNode *>(fade->firstChild());
    } else {
        node = new QSGNode;
        move = new QSGTransformNode;
        node->appendChildNode(move);
        drawn = new QSGGeometryNode;
        drawn->setGeometry(new QSGGeometry(vertexAttributes(), 0));
        drawn->geometry()->setDrawingMode(QSGGeometry::DrawTriangles);
        drawn->geometry()->setVertexDataPattern(QSGGeometry::StreamPattern);
        drawn->setFlag(QSGNode::OwnsGeometry);
        drawn->setMaterial(new RkGlyphMaterial);
        drawn->setFlag(QSGNode::OwnsMaterial);
        move->appendChildNode(drawn);
        content = shown->createImageNode();
        content->setOwnsTexture(true);
        node->appendChildNode(content);
        fade = new QSGOpacityNode;
        node->appendChildNode(fade);
        cursor = shown->createImageNode();
        cursor->setOwnsTexture(true);
        fade->appendChildNode(cursor);
        m_contentDirty = true;
        m_shownOnGpu = false;
    }
    // The images lie on whole pixels of the device. At a fractional scale, as 125 %, the item can
    // start or end between two of them; its texture would then be stretched or sampled between
    // pixels, and the text blurred or shifted by a pixel from some column on.
    const QPointF corner = mapToScene(QPointF());
    const QPointF snap(qRound(corner.x() * ratio) / ratio - corner.x(), qRound(corner.y() * ratio) / ratio - corner.y());
    const QRectF place(snap, QSizeF(size) / ratio);
    // On the graphics card, each frame is whole: it costs little there, and the image that
    // would hold the frame before is not kept.
    const bool gpu = shown->rhi() && drawsOnGpu();
    if (gpu != m_gpu || (gpu && m_linesDirty)) {
        m_contentDirty = true;
    }
    m_gpu = gpu;
    // Only the lines that changed, into the image that holds the frame before, where the item
    // can tell them; else, or with nothing of the frame kept, all of it.
    bool linesPainted = false;
    if (m_linesDirty && !m_contentDirty && m_image.size() == size) {
        int painted = NoLines;
        QRegion bands;
        {
            QPainter painter(&m_image);
            t_bands = &bands;
            painted = paintLines(&painter);
            t_bands = nullptr;
        }
        if (painted == WholeFrame) {
            m_contentDirty = true;
        } else if (painted == SomeLines || painted == AllLines) {
            showFrame(shown, content, m_image, m_opaque, place, size, smooth(), bands, painted == AllLines);
            linesPainted = true;
        }
    }
    m_linesDirty = false;
    const bool alone = !m_contentDirty && m_shownSize == size;
    if (!alone) {
        bool onGpu = false;
        if (gpu) {
            auto material = static_cast<RkGlyphMaterial *>(drawn->material());
            material->atlas.begin(ratio, shown->rhi()->resourceLimit(QRhi::TextureSizeMax));
            RkGpuFrame frame{&material->atlas, ratio, drawn->geometry(), 0, material->shown, material->atlas.size()};
#ifdef RK_TEST_HOOKS
            makeRoom(frame, 0);
#endif
            // The fonts of the frame resolve against the density of pixels of what the
            // painter paints on, here an image that takes nothing.
            if (m_probe.isNull() || m_probe.devicePixelRatio() != ratio) {
                m_probe = QImage(1, 1, QImage::Format_ARGB32_Premultiplied);
                m_probe.setDevicePixelRatio(ratio);
            }
            {
                QPainter painter(&m_probe);
                t_gpu = &frame;
                paint(&painter);
                t_gpu = nullptr;
            }
            if (!frame.failed) {
                showGpuFrame(drawn, frame);
                material->shown = frame.used;
                onGpu = true;
            } else {
                gpuFallback();
            }
        }
        if (onGpu) {
            if (!m_shownOnGpu) {
                // Neither the image nor its texture is needed while frames are drawn there.
                m_image = QImage();
                QImage nothing(1, 1, QImage::Format_ARGB32_Premultiplied);
                nothing.fill(Qt::transparent);
                showFrame(shown, content, nothing, false, QRectF(), QSize(1, 1), false, QRegion(), true);
            }
        } else {
            if (m_image.size() != size) {
                m_image = QImage(size, QImage::Format_ARGB32_Premultiplied);
            }
            m_image.setDevicePixelRatio(ratio);
            // An opaque terminal paints its background over all of it first.
            if (!m_opaque) {
                m_image.fill(Qt::transparent);
            }
            {
                QPainter painter(&m_image);
                paint(&painter);
            }
            showFrame(shown, content, m_image, m_opaque, place, size, smooth(), QRegion(), true);
            if (drawn->geometry()->vertexCount() > 0) {
                static_cast<RkGlyphMaterial *>(drawn->material())->shown = 0;
                drawn->geometry()->allocate(0);
                drawn->markDirty(QSGNode::DirtyGeometry);
            }
        }
        m_shownOnGpu = onGpu;
        m_shownSize = size;
        m_contentDirty = false;
    } else if (!m_shownOnGpu && content->rect() != place) {
        // The item moved by a part of a pixel, with the same image.
        content->setRect(place);
    }
    QMatrix4x4 shift;
    shift.translate(float(snap.x()), float(snap.y()));
    if (move->matrix() != shift) {
        move->setMatrix(shift);
    }
    const QSize room = (cursorRoom() * ratio).toSize().expandedTo(QSize(1, 1));
    // A new image, or a new node, has to be painted; otherwise the cursor may keep its texture.
    const bool mayKeep = m_cursor.size() == room && cursor->texture() != nullptr;
    if (m_cursor.size() != room) {
        m_cursor = QImage(room, QImage::Format_ARGB32_Premultiplied);
    }
    m_cursor.setDevicePixelRatio(ratio);
    m_cursor.fill(Qt::transparent);
    QRectF where;
    bool kept = false;
    {
        QPainter painter(&m_cursor);
        where = paintCursor(&painter, alone && !linesPainted, mayKeep, kept);
    }
    const double opacity = where.isEmpty() ? 0.0 : cursorOpacity();
    fade->setOpacity(opacity);
    if (!where.isEmpty()) {
        const QSize covered = (where.size() * ratio).toSize().boundedTo(room);
        const QRectF at(where.topLeft() + snap, QSizeF(covered) / ratio);
        if (!kept) {
            showImage(shown, cursor, m_cursor, false, at, covered, smooth());
        } else if (cursor->rect() != at) {
            // The item moved by a part of a pixel, with the same cursor.
            cursor->setRect(at);
        }
    } else if (!cursor->texture()) {
        // A node without a texture is not drawn, but it must have one to exist at all.
        showImage(shown, cursor, m_cursor, false, QRectF(), QSize(1, 1), smooth());
    }
    return node;
}

// Positions on the baseline. One run per font and color spares the text layout that
// `QPainter::drawText` does for every call.
void rk_draw_text(QPainter &painter, const QPointF &position, const QString &text)
{
    // Only the painter draws this: a frame on the graphics card is painted instead.
    if (t_gpu) {
        t_gpu->failed = true;
    }
    painter.drawText(position, text);
}

void rk_draw_glyphs(QPainter &painter,
                    const QRawFont &font,
                    const QColor &color,
                    rust::Slice<const std::uint32_t> glyphs,
                    rust::Slice<const QPointF> positions)
{
    const std::size_t count = qMin(glyphs.size(), positions.size());
    // On the steps of a sixty-fourth of a pixel that `drawText` places glyphs on: a bitmap of
    // its own colors at a fractional scale lands a pixel apart otherwise.
    QVarLengthArray<QPointF, 256> places;
    places.reserve(qsizetype(count));
    for (std::size_t i = 0; i < count; ++i) {
        places.append(QPointF(std::floor(positions[i].x() * 64) / 64, std::floor(positions[i].y() * 64) / 64));
    }
    QGlyphRun run;
    run.setRawFont(font);
    run.setRawData(glyphs.data(), places.constData(), int(places.size()));
    // Qt keeps the glyphs of a color font, as emoji, once for each color of the pen, though
    // their bitmaps have colors of their own: one pen for them, so that they come from the
    // cache rather than from the font each time a color is new.
    const QFontEngine *engine = QRawFontPrivate::get(font)->fontEngine;
    painter.setPen(engine && engine->isColorFont() ? QColor(Qt::white) : color);
    painter.drawGlyphRun(QPointF(), run);
}

void rk_font_family(QFont &font, const QString &family)
{
    font.setFamilies({family, font.family()});
}

QString rk_color_from_text(const QString &text)
{
    const QColor color = QColor::fromString(text);
    return color.isValid() ? color.name() : QString();
}

void rk_clipboard_set_rich(const QString &text, const QString &html)
{
    auto data = new QMimeData;
    data->setText(text);
    if (!html.isEmpty()) {
        data->setHtml(html);
    }
    QGuiApplication::clipboard()->setMimeData(data, QClipboard::Clipboard);
}

void rk_ask_save_file(int terminal, int purpose, const QString &suggestion)
{
    auto *dialog = new QFileDialog;
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setAcceptMode(QFileDialog::AcceptSave);
    dialog->selectFile(suggestion);
    QObject::connect(dialog, &QDialog::finished, [dialog, terminal, purpose](int result) {
        qt_dialog_active(false);
        const QStringList files = dialog->selectedFiles();
        if (result == QDialog::Accepted && !files.isEmpty()) {
            qt_file_chosen(terminal, purpose, files.constFirst());
        }
    });
    qt_dialog_active(true);
    dialog->open();
}

void rk_print_text(const QString &text)
{
    auto *printer = new QPrinter;
    auto *dialog = new QPrintDialog(printer);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    // The dialog uses the printer until it is gone.
    QObject::connect(dialog, &QObject::destroyed, [printer] {
        delete printer;
    });
    QObject::connect(dialog, &QDialog::finished, [printer, text](int result) {
        qt_dialog_active(false);
        if (result == QDialog::Accepted) {
            QTextDocument document(text);
            document.setDefaultFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
            document.print(printer);
        }
    });
    qt_dialog_active(true);
    dialog->open();
}

void rk_ask_history_lines(int terminal, int current, const QString &title, const QString &label)
{
    auto *dialog = new QInputDialog;
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(title);
    dialog->setLabelText(label);
    dialog->setInputMode(QInputDialog::IntInput);
    dialog->setIntRange(0, 1000000);
    dialog->setIntStep(1000);
    dialog->setIntValue(current);
    QObject::connect(dialog, &QDialog::finished, [dialog, terminal](int result) {
        qt_dialog_active(false);
        if (result == QDialog::Accepted) {
            qt_history_lines_chosen(terminal, dialog->intValue());
        }
    });
    qt_dialog_active(true);
    dialog->open();
}

QString rk_screen_names()
{
    QStringList names;
    const auto screens = QGuiApplication::screens();
    for (const QScreen *screen : screens) {
        names << screen->name();
    }
    return names.join(QLatin1Char(','));
}

QString rk_pointer_screen()
{
    const QScreen *screen = QGuiApplication::screenAt(QCursor::pos());
    return screen ? screen->name() : QString();
}

QString rk_primary_screen()
{
    const QScreen *screen = QGuiApplication::primaryScreen();
    return screen ? screen->name() : QString();
}

QString rk_screen_available_size(const QString &name)
{
    const auto screens = QGuiApplication::screens();
    for (const QScreen *screen : screens) {
        if (screen->name() == name) {
            const QRect area = screen->availableGeometry();
            return QStringLiteral("%1,%2").arg(area.width()).arg(area.height());
        }
    }
    return {};
}

// The 15 colors of `theme::parse_palette` from the palette of Qt, which follows the system.
// Qt has no colors for good, bad and neutral news; those are the ones of Fluent.
QString rk_palette_colors()
{
    const QPalette palette = QGuiApplication::palette();
    const auto name = [&palette](QPalette::ColorRole role) {
        return palette.color(QPalette::Active, role).name();
    };
    const bool dark = palette.color(QPalette::Window).lightness() < 128;
    return QStringList{
        name(QPalette::Window),
        name(QPalette::WindowText),
        name(QPalette::Window),
        name(QPalette::WindowText),
        name(QPalette::Highlight),
        name(QPalette::HighlightedText),
        dark ? QStringLiteral("#ff99a4") : QStringLiteral("#c42b1c"),
        name(QPalette::Base),
        name(QPalette::Text),
        dark ? QStringLiteral("#6ccb5f") : QStringLiteral("#0f7b0f"),
        dark ? QStringLiteral("#fce100") : QStringLiteral("#9d5d00"),
        name(QPalette::Link),
        name(QPalette::LinkVisited),
        name(QPalette::Accent),
        name(QPalette::PlaceholderText),
    }
        .join(QLatin1Char(','));
}

// The caller checks the target with `links::may_open`. A path becomes a file address as it
// is; nothing here guesses what text might mean.
void rk_open_url_with_qt(const QString &target)
{
    const QUrl url = QDir::isAbsolutePath(target) ? QUrl::fromLocalFile(target) : QUrl(target, QUrl::StrictMode);
    if (url.isValid() && !(url.isLocalFile() && !url.host().isEmpty())) {
        QDesktopServices::openUrl(url);
    }
}

QString rk_ui_languages()
{
    return QLocale::system().uiLanguages().join(QLatin1Char(','));
}

int rk_base_key(int key, int modifiers, int scanCode, const QString &text)
{
    quint32 virtualKey = 0;
#ifdef Q_OS_WIN
    // The key mapper of Windows looks the key up by its virtual key, which QML does not
    // pass on: it follows from the scan code, with the prefix of an extended key.
    const UINT scan = (scanCode & 0x100) ? (0xE000u | (UINT(scanCode) & 0xFFu)) : UINT(scanCode);
    virtualKey = MapVirtualKeyW(scan, MAPVK_VSC_TO_VK_EX);
#endif
    const QKeyEvent event(QEvent::KeyPress, key, Qt::KeyboardModifiers(modifiers), quint32(scanCode), virtualKey, 0, text);
    // The first combination is the key on its first level, with all the modifiers.
    const QList<QKeyCombination> possible = QKeyMapper::possibleKeys(&event);
    return possible.isEmpty() ? key : int(possible.first().key());
}

QString rk_key_sequence_text(int key, int modifiers)
{
    const QKeyCombination combination(Qt::KeyboardModifiers(modifiers) & ~Qt::KeypadModifier, Qt::Key(key));
    return QKeySequence(combination).toString(QKeySequence::PortableText);
}

QString rk_native_key_sequence(const QString &portable)
{
    return QKeySequence::fromString(portable, QKeySequence::PortableText).toString(QKeySequence::NativeText);
}

// How long a hidden window keeps its scene graph, in milliseconds; 0 for ever.
static int s_releaseAfter = 0;

void rk_release_after(int milliseconds)
{
    s_releaseAfter = qMax(0, milliseconds);
}

// A window hidden for `s_releaseAfter` gives up its scene graph and, with it, its graphics,
// its render thread and the images of its items; showing it again keeps them until the next
// time. Each window once, as the front end calls this after it loaded them.
void rk_release_when_hidden()
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *window : windows) {
        auto quick = qobject_cast<QQuickWindow *>(window);
        if (!quick || quick->property("rkReleaseWhenHidden").toBool()) {
            continue;
        }
        quick->setProperty("rkReleaseWhenHidden", true);
        auto timer = new QTimer(quick);
        timer->setSingleShot(true);
        QObject::connect(timer, &QTimer::timeout, quick, [quick] {
            if (quick->isVisible()) {
                return;
            }
            quick->setPersistentSceneGraph(false);
            quick->setPersistentGraphics(false);
            quick->releaseResources();
#ifdef __GLIBC__
            // The render thread frees the images when it ends, a moment later; the allocator
            // of glibc keeps what was freed for the process unless asked to give it back.
            QTimer::singleShot(1000, quick, [] { malloc_trim(0); });
#endif
        });
        QObject::connect(quick, &QWindow::visibleChanged, timer, [quick, timer](bool visible) {
            if (visible) {
                timer->stop();
                quick->setPersistentGraphics(true);
                quick->setPersistentSceneGraph(true);
            } else if (s_releaseAfter > 0) {
                timer->start(s_releaseAfter);
            }
        });
    }
}
