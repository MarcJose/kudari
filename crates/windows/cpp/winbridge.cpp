// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

#include "winbridge.h"
#include "kudari-windows/src/win.cxx.h"

#include <QAction>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QIcon>
#include <QKeySequence>
#include <QLocalServer>
#include <QLocalSocket>
#include <QMenu>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QStyleHints>
#include <QSurfaceFormat>
#include <QTimer>
#include <QSystemTrayIcon>

#ifdef Q_OS_WIN
#include <string>

#include <dwmapi.h>
#include <windows.h>
#endif

namespace
{
QSystemTrayIcon *s_tray = nullptr;

#ifdef Q_OS_WIN
constexpr int HOTKEY_ID = 1;
constexpr UINT TOGGLE_MESSAGE = WM_APP + 1;
HWND s_window = nullptr;

// The hidden window of this instance: it takes the global shortcut and the request of
// another start to toggle the window.
LRESULT CALLBACK instanceProcedure(HWND window, UINT message, WPARAM wParam, LPARAM lParam)
{
    if (message == TOGGLE_MESSAGE || (message == WM_HOTKEY && wParam == HOTKEY_ID)) {
        win_hotkey_pressed();
        return 0;
    }
    return DefWindowProcW(window, message, wParam, lParam);
}

// The virtual key of Windows for a key of Qt, for the keys a global shortcut is likely to use.
UINT virtualKey(Qt::Key key)
{
    if (key >= Qt::Key_F1 && key <= Qt::Key_F24) {
        return VK_F1 + static_cast<UINT>(key - Qt::Key_F1);
    }
    if ((key >= Qt::Key_A && key <= Qt::Key_Z) || (key >= Qt::Key_0 && key <= Qt::Key_9)) {
        return static_cast<UINT>(key);
    }
    switch (key) {
    case Qt::Key_QuoteLeft:
        return VK_OEM_3;
    case Qt::Key_Space:
        return VK_SPACE;
    case Qt::Key_Insert:
        return VK_INSERT;
    case Qt::Key_Pause:
        return VK_PAUSE;
    case Qt::Key_ScrollLock:
        return VK_SCROLL;
    case Qt::Key_Escape:
        return VK_ESCAPE;
    default:
        return 0;
    }
}
#else
QLocalServer *s_server = nullptr;
#endif

// The icon of Kudari in the sizes that tools/icons.py renders, for windows and the tray.
QIcon appIcon()
{
    QIcon icon;
    for (const int size : {16, 20, 24, 32, 40, 48, 64, 96, 256}) {
        icon.addFile(QStringLiteral(":/icons/kudari-%1.png").arg(size), QSize(size, size));
    }
    return icon;
}
}

#ifdef Q_OS_WIN
// Another start of Kudari toggles this one. A mutex in the namespace of the login session
// tells that one runs, which users of other sessions cannot take; the other start then only
// posts a message to the hidden window of this one. A named pipe would let whoever created
// it first act with the rights of the user who connects.
bool rk_win_claim_instance(const QString &name)
{
    const std::wstring className = name.toStdWString();
    HANDLE mutex = CreateMutexW(nullptr, FALSE, (L"Local\\" + className).c_str());
    if (mutex && GetLastError() == ERROR_ALREADY_EXISTS) {
        CloseHandle(mutex);
        if (HWND other = FindWindowExW(HWND_MESSAGE, nullptr, className.c_str(), nullptr)) {
            PostMessageW(other, TOGGLE_MESSAGE, 0, 0);
        }
        return false;
    }
    // The mutex stays open for as long as this instance runs.
    WNDCLASSW windowClass{};
    windowClass.lpfnWndProc = instanceProcedure;
    windowClass.hInstance = GetModuleHandleW(nullptr);
    windowClass.lpszClassName = className.c_str();
    RegisterClassW(&windowClass);
    s_window = CreateWindowExW(0, className.c_str(), L"", 0, 0, 0, 0, 0, HWND_MESSAGE, nullptr, windowClass.hInstance, nullptr);
    return true;
}

bool rk_win_register_hotkey(const QString &sequence)
{
    if (!s_window) {
        return false;
    }
    UnregisterHotKey(s_window, HOTKEY_ID);
    if (sequence.isEmpty()) {
        return true;
    }
    const QKeyCombination combination = QKeySequence(sequence)[0];
    const UINT key = virtualKey(combination.key());
    if (key == 0) {
        return false;
    }
    const Qt::KeyboardModifiers modifiers = combination.keyboardModifiers();
    UINT flags = MOD_NOREPEAT;
    flags |= modifiers.testFlag(Qt::ControlModifier) ? MOD_CONTROL : 0;
    flags |= modifiers.testFlag(Qt::AltModifier) ? MOD_ALT : 0;
    flags |= modifiers.testFlag(Qt::ShiftModifier) ? MOD_SHIFT : 0;
    flags |= modifiers.testFlag(Qt::MetaModifier) ? MOD_WIN : 0;
    return RegisterHotKey(s_window, HOTKEY_ID, flags, key) != 0;
}
#else
// Elsewhere, where this front end only runs to be tried out, a local socket at `name`, a
// path in the private runtime directory of the user, does the same.
bool rk_win_claim_instance(const QString &name)
{
    QLocalSocket other;
    other.connectToServer(name);
    if (other.waitForConnected(500)) {
        other.write("toggle\n");
        other.waitForBytesWritten(500);
        return false;
    }
    s_server = new QLocalServer(QCoreApplication::instance());
    s_server->setSocketOptions(QLocalServer::UserAccessOption);
    QLocalServer::removeServer(name);
    if (!s_server->listen(name)) {
        return true;
    }
    QObject::connect(s_server, &QLocalServer::newConnection, s_server, [] {
        while (QLocalSocket *socket = s_server->nextPendingConnection()) {
            QObject::connect(socket, &QLocalSocket::readyRead, socket, [socket] {
                if (socket->readAll().startsWith("toggle")) {
                    win_hotkey_pressed();
                }
                socket->disconnectFromServer();
            });
            QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
        }
    });
    return true;
}

// There is no global shortcut to register without Windows.
bool rk_win_register_hotkey(const QString &)
{
    return true;
}
#endif

// For checks: saves the window with its menus and dialogs as an image after `delay`
// milliseconds, and quits.
void rk_win_screenshot_later(const QString &path, int delay)
{
    QTimer::singleShot(delay, QCoreApplication::instance(), [path] {
        const auto windows = QGuiApplication::topLevelWindows();
        for (QWindow *window : windows) {
            auto quick = qobject_cast<QQuickWindow *>(window);
            if (quick && quick->isVisible()) {
                quick->grabWindow().save(path);
                break;
            }
        }
        QCoreApplication::quit();
    });
}

void rk_win_attach_console()
{
#ifdef Q_OS_WIN
    // A program without a console writes nowhere unless the caller redirected its output.
    if (!GetStdHandle(STD_OUTPUT_HANDLE) && AttachConsole(ATTACH_PARENT_PROCESS)) {
        FILE *stream = nullptr;
        freopen_s(&stream, "CONOUT$", "w", stdout);
        freopen_s(&stream, "CONOUT$", "w", stderr);
    }
#endif
}

// Windows that shows through needs an alpha channel, before the first window exists.
void rk_win_prepare_translucency()
{
    QSurfaceFormat format = QSurfaceFormat::defaultFormat();
    format.setAlphaBufferSize(8);
    QSurfaceFormat::setDefaultFormat(format);
}

// Qt asks Windows for light or dark again with Unknown.
void rk_win_set_color_scheme(const QString &scheme)
{
    Qt::ColorScheme wanted = Qt::ColorScheme::Unknown;
    if (scheme == QLatin1String("light")) {
        wanted = Qt::ColorScheme::Light;
    } else if (scheme == QLatin1String("dark")) {
        wanted = Qt::ColorScheme::Dark;
    }
    QGuiApplication::styleHints()->setColorScheme(wanted);
}

// The icon of the windows, and the style of Windows 11 for the controls, unless
// QT_QUICK_CONTROLS_STYLE names another.
// Setting that variable from Rust would not do: on Windows it changes the environment of the
// process, while Qt reads the copy of the C runtime.
void rk_win_use_style()
{
    QGuiApplication::setWindowIcon(appIcon());
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE")) {
        QQuickStyle::setStyle(QStringLiteral("FluentWinUI3"));
    }
}

namespace
{
// Tells about changes of the light or dark scheme and of the colors of Windows, which Qt
// takes over into its palette.
class ThemeWatcher : public QObject
{
public:
    using QObject::QObject;

protected:
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::ApplicationPaletteChange) {
            win_theme_changed();
        }
        return QObject::eventFilter(watched, event);
    }
};

bool s_blurred = false;
}

void rk_win_watch_theme()
{
    auto *watcher = new ThemeWatcher(QCoreApplication::instance());
    QCoreApplication::instance()->installEventFilter(watcher);
    QObject::connect(QGuiApplication::styleHints(), &QStyleHints::colorSchemeChanged, watcher, [] {
        rk_win_set_backdrop(s_blurred);
        win_theme_changed();
    });
}

// Behind a terminal that shows through, Windows 11 blurs what lies behind the window with
// its Acrylic backdrop, in the light or dark of the scheme; older Windows ignores it.
void rk_win_set_backdrop(bool blurred)
{
    s_blurred = blurred;
#ifdef Q_OS_WIN
    constexpr DWORD USE_IMMERSIVE_DARK_MODE = 20;
    constexpr DWORD SYSTEMBACKDROP_TYPE = 38;
    constexpr int BACKDROP_NONE = 1;
    constexpr int BACKDROP_ACRYLIC = 3;
    const BOOL dark = QGuiApplication::styleHints()->colorScheme() == Qt::ColorScheme::Dark;
    const int backdrop = blurred ? BACKDROP_ACRYLIC : BACKDROP_NONE;
    const MARGINS sheet{-1, -1, -1, -1};
    const MARGINS none{0, 0, 0, 0};
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *window : windows) {
        if (!qobject_cast<QQuickWindow *>(window)) {
            continue;
        }
        const auto handle = reinterpret_cast<HWND>(window->winId());
        DwmSetWindowAttribute(handle, USE_IMMERSIVE_DARK_MODE, &dark, sizeof dark);
        DwmExtendFrameIntoClientArea(handle, blurred ? &sheet : &none);
        DwmSetWindowAttribute(handle, SYSTEMBACKDROP_TYPE, &backdrop, sizeof backdrop);
    }
#endif
}

void rk_win_setup_tray(const QString &toggleText, const QString &quitText)
{
    if (s_tray || !QSystemTrayIcon::isSystemTrayAvailable()) {
        return;
    }
    s_tray = new QSystemTrayIcon(appIcon(), QCoreApplication::instance());
    s_tray->setToolTip(QStringLiteral("Kudari"));
    auto *menu = new QMenu;
    QObject::connect(menu->addAction(toggleText), &QAction::triggered, menu, [] {
        win_hotkey_pressed();
    });
    menu->addSeparator();
    QObject::connect(menu->addAction(quitText), &QAction::triggered, menu, [] {
        win_quit_requested();
    });
    s_tray->setContextMenu(menu);
    QObject::connect(s_tray, &QSystemTrayIcon::activated, s_tray, [](QSystemTrayIcon::ActivationReason reason) {
        if (reason == QSystemTrayIcon::Trigger) {
            win_hotkey_pressed();
        }
    });
}

void rk_win_set_tray_visible(bool visible)
{
    if (s_tray) {
        s_tray->setVisible(visible);
    }
}

void rk_win_notify(const QString &title, const QString &text)
{
    if (s_tray && s_tray->isVisible()) {
        s_tray->showMessage(title, text, s_tray->icon());
    }
}
