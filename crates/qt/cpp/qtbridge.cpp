// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

// What the front ends need of Qt itself: the clipboard, fonts, the badge, the input method
// and the dialogs for files, printing and the scrollback.
#include "qtbridge.h"
#include "kudari-qt/src/qt.cxx.h"

#include <QClipboard>
#include <cstring>
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
#include <QInputDialog>
#include <QInputMethodEvent>
#include <QKeySequence>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMimeData>
#include <QPainter>
#include <QPrintDialog>
#include <QPrinter>
#include <QQuickWindow>
#include <QRegion>
#include <QSGImageNode>
#include <QSGNode>
#include <QSGTexture>
#include <QTextDocument>
#include <QTimer>
#ifdef __GLIBC__
#include <malloc.h>
#endif
#include <QVarLengthArray>
// Private, as nothing public names the key of a key event on its first level.
#include <QtGui/private/qkeymapper_p.h>
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
        return nullptr;
    }
    // The frame first, then the cursor over it, which fades through an opacity of its own.
    QSGImageNode *content = nullptr;
    QSGOpacityNode *fade = nullptr;
    QSGImageNode *cursor = nullptr;
    if (node) {
        content = static_cast<QSGImageNode *>(node->firstChild());
        fade = static_cast<QSGOpacityNode *>(content->nextSibling());
        cursor = static_cast<QSGImageNode *>(fade->firstChild());
    } else {
        node = new QSGNode;
        content = shown->createImageNode();
        content->setOwnsTexture(true);
        node->appendChildNode(content);
        fade = new QSGOpacityNode;
        node->appendChildNode(fade);
        cursor = shown->createImageNode();
        cursor->setOwnsTexture(true);
        fade->appendChildNode(cursor);
        m_contentDirty = true;
    }
    // The images lie on whole pixels of the device. At a fractional scale, as 125 %, the item can
    // start or end between two of them; its texture would then be stretched or sampled between
    // pixels, and the text blurred or shifted by a pixel from some column on.
    const QPointF corner = mapToScene(QPointF());
    const QPointF snap(qRound(corner.x() * ratio) / ratio - corner.x(), qRound(corner.y() * ratio) / ratio - corner.y());
    const QRectF place(snap, QSizeF(size) / ratio);
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
    const bool alone = !m_contentDirty && m_image.size() == size;
    if (!alone) {
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
        m_contentDirty = false;
    } else if (content->rect() != place) {
        // The item moved by a part of a pixel, with the same image.
        content->setRect(place);
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
    painter.drawText(position, text);
}

void rk_draw_glyphs(QPainter &painter,
                    const QRawFont &font,
                    const QColor &color,
                    rust::Slice<const std::uint32_t> glyphs,
                    rust::Slice<const QPointF> positions)
{
    QGlyphRun run;
    run.setRawFont(font);
    run.setRawData(glyphs.data(), positions.data(), int(qMin(glyphs.size(), positions.size())));
    painter.setPen(color);
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
