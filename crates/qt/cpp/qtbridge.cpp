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
#include <QTextDocument>
// Private, as nothing public names the key of a key event on its first level.
#include <QtGui/private/qkeymapper_p.h>
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

// Qt Quick has an item paint into an image in RGBA8888, for which the raster engine of Qt has
// no fast way to draw text: it converts every pixel of every glyph to 64 bits and back. Into
// ARGB32_Premultiplied it draws text several times faster, so a frame goes there first and
// then to the item in one piece. One image per thread serves all terminals of a window: it
// grows to the largest of them, and each draws into its top left corner. It shrinks to the
// frame again once it has been more than twice as large for a while, as after full screen,
// and not for a single smaller terminal among larger ones. Null when the target has no size.
std::unique_ptr<RkFrame> rk_frame_begin(QPainter &target, bool opaque)
{
    // Frames in a row for which the image was more than twice as large as needed.
    constexpr int roomyFrames = 120;
    thread_local QImage image;
    thread_local int roomy = 0;
    QPaintDevice *device = target.device();
    if (device == nullptr || device->width() <= 0 || device->height() <= 0) {
        return nullptr;
    }
    const QSize size(device->width(), device->height());
    const qint64 needed = qint64(size.width()) * size.height();
    if (image.width() < size.width() || image.height() < size.height()) {
        image = QImage(size.expandedTo(image.size()), QImage::Format_ARGB32_Premultiplied);
        roomy = 0;
    } else if (qint64(image.width()) * image.height() > 2 * needed) {
        if (++roomy > roomyFrames) {
            image = QImage(size, QImage::Format_ARGB32_Premultiplied);
            roomy = 0;
        }
    } else {
        roomy = 0;
    }
    image.setDevicePixelRatio(device->devicePixelRatioF());
    // An opaque terminal paints its background over all of it first.
    if (!opaque) {
        for (int y = 0; y < size.height(); ++y) {
            std::memset(image.scanLine(y), 0, size_t(size.width()) * 4);
        }
    }
    auto frame = std::make_unique<RkFrame>();
    frame->image = &image;
    frame->size = size;
    frame->painter.begin(&image);
    frame->painter.setTransform(target.transform());
    frame->painter.setRenderHints(target.renderHints());
    return frame;
}

QPainter &rk_frame_painter(RkFrame &frame)
{
    return frame.painter;
}

void rk_frame_end(QPainter &target, std::unique_ptr<RkFrame> frame, bool opaque)
{
    frame->painter.end();
    target.save();
    target.resetTransform();
    if (opaque) {
        target.setCompositionMode(QPainter::CompositionMode_Source);
    }
    target.drawImage(QPointF(), *frame->image, QRectF(QPointF(), frame->size));
    target.restore();
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
