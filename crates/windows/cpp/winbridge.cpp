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
#include <QLoggingCategory>
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
#include <appmodel.h>
#include <roapi.h>
#include <windows.data.xml.dom.h>
#include <windows.ui.notifications.h>
#include <wrl/client.h>
#include <wrl/wrappers/corewrappers.h>
#endif

// The messages handed to the tray, for the GUI test: off unless QT_LOGGING_RULES turns it on,
// as the notifications of Windows can be off.
Q_LOGGING_CATEGORY(lcNotify, "kudari.notify", QtWarningMsg)

namespace
{
QSystemTrayIcon *s_tray = nullptr;
// Whether the user wants the icon in the tray; without it, it shows for a message alone.
bool s_trayWanted = true;
// Hides the icon again once a message that needed it has shown.
QTimer *s_trayHide = nullptr;

#ifdef Q_OS_WIN
constexpr int HOTKEY_ID = 1;

// The key that types `character` with the layout of the keyboard now, or 0 if none does.
UINT keyOfCharacter(wchar_t character)
{
    const SHORT found = VkKeyScanExW(character, GetKeyboardLayout(0));
    return found == -1 ? 0 : static_cast<UINT>(found & 0xff);
}
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

// The virtual key of Windows for a key of the number pad, which has keys of its own for the
// digits and for arithmetic; 0 for the others, which are the same keys as elsewhere.
UINT keypadKey(Qt::Key key)
{
    if (key >= Qt::Key_0 && key <= Qt::Key_9) {
        return VK_NUMPAD0 + static_cast<UINT>(key - Qt::Key_0);
    }
    switch (key) {
    case Qt::Key_Asterisk:
        return VK_MULTIPLY;
    case Qt::Key_Plus:
        return VK_ADD;
    case Qt::Key_Minus:
        return VK_SUBTRACT;
    case Qt::Key_Slash:
        return VK_DIVIDE;
    case Qt::Key_Period:
    case Qt::Key_Comma:
        return VK_DECIMAL;
    default:
        return 0;
    }
}

// The virtual key of Windows for a key of Qt, for the keys a global shortcut is likely to use.
UINT virtualKey(QKeyCombination combination)
{
    const Qt::Key key = combination.key();
    if (combination.keyboardModifiers().testFlag(Qt::KeypadModifier)) {
        if (const UINT keypad = keypadKey(key)) {
            return keypad;
        }
    }
    if (key >= Qt::Key_F1 && key <= Qt::Key_F24) {
        return VK_F1 + static_cast<UINT>(key - Qt::Key_F1);
    }
    if ((key >= Qt::Key_A && key <= Qt::Key_Z) || (key >= Qt::Key_0 && key <= Qt::Key_9)) {
        return static_cast<UINT>(key);
    }
    switch (key) {
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
    case Qt::Key_Home:
        return VK_HOME;
    case Qt::Key_End:
        return VK_END;
    case Qt::Key_PageUp:
        return VK_PRIOR;
    case Qt::Key_PageDown:
        return VK_NEXT;
    case Qt::Key_Left:
        return VK_LEFT;
    case Qt::Key_Up:
        return VK_UP;
    case Qt::Key_Right:
        return VK_RIGHT;
    case Qt::Key_Down:
        return VK_DOWN;
    case Qt::Key_Tab:
    case Qt::Key_Backtab:
        return VK_TAB;
    case Qt::Key_Return:
    case Qt::Key_Enter:
        return VK_RETURN;
    case Qt::Key_Backspace:
        return VK_BACK;
    case Qt::Key_Delete:
        return VK_DELETE;
    case Qt::Key_Print:
        return VK_SNAPSHOT;
    case Qt::Key_CapsLock:
        return VK_CAPITAL;
    case Qt::Key_NumLock:
        return VK_NUMLOCK;
    case Qt::Key_Menu:
        return VK_APPS;
    // Dead keys, as ^ left of 1 on a German keyboard, by the character they put on letters.
    case Qt::Key_Dead_Circumflex:
        return keyOfCharacter(L'^');
    case Qt::Key_Dead_Grave:
        return keyOfCharacter(L'`');
    case Qt::Key_Dead_Acute:
        return keyOfCharacter(L'\u00b4');
    default:
        // Other characters, such as ^, ` or ß, lie where the layout of the keyboard puts them:
        // ` is left of 1 in the United States, and next to the backspace in Germany.
        return key > Qt::Key_Space && key <= 0xff ? keyOfCharacter(static_cast<wchar_t>(key)) : 0;
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
    const DWORD error = GetLastError();
    if (mutex && error == ERROR_ALREADY_EXISTS) {
        CloseHandle(mutex);
        if (HWND other = FindWindowExW(HWND_MESSAGE, nullptr, className.c_str(), nullptr)) {
            // The user started this one, so it may bring a window to the front; the other
            // instance runs in the background and needs that right to take the keyboard.
            DWORD process = 0;
            GetWindowThreadProcessId(other, &process);
            if (process) {
                AllowSetForegroundWindow(process);
            }
            PostMessageW(other, TOGGLE_MESSAGE, 0, 0);
        }
        return false;
    }
    if (!mutex && error == ERROR_ACCESS_DENIED) {
        // An instance with more rights runs, such as one run as administrator. Windows
        // keeps its window from messages of this one, and a second instance must not start.
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

bool rk_win_hotkey_supported(const QString &sequence)
{
    return sequence.isEmpty() || virtualKey(QKeySequence(sequence)[0]) != 0;
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
    const UINT key = virtualKey(combination);
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
bool rk_win_hotkey_supported(const QString &)
{
    return true;
}

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

// Libraries load only from the folder of the program, from System32 and from the folder of
// Qt, not from the working directory or PATH. `alacritty_terminal` loads `conpty.dll` by name
// for every new terminal, and Windows would otherwise take one from any folder of PATH, or
// from the folder Kudari was started in, and run it with the rights of Kudari. A newer
// `conpty.dll` goes next to `kudari.exe`. First thing in main, before anything loads a library.
void rk_win_harden_dll_search()
{
#ifdef Q_OS_WIN
    SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    // Takes the working directory out of the search for the calls that bypass the default.
    SetDllDirectoryW(L"");
    // Qt loads its plugins later, and they need more of Qt. In the packages, Qt lies next to
    // kudari.exe; in a build of cargo, where PATH names the folder of Qt, it does not. That
    // folder stays in the search: the one Qt itself was loaded from, which runs already.
    HMODULE qt = nullptr;
    if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCWSTR>(&qVersion),
                           &qt)) {
        std::wstring path(32768, L'\0');
        const DWORD length = GetModuleFileNameW(qt, path.data(), DWORD(path.size()));
        const auto folder = path.find_last_of(L'\\', length);
        if (length > 0 && length < path.size() && folder != std::wstring::npos) {
            path.resize(folder);
            AddDllDirectory(path.c_str());
        }
    }
#endif
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
    s_trayWanted = visible;
    // An icon that shows for a message stays until the message has shown.
    if (s_tray && (visible || !s_trayHide || !s_trayHide->isActive())) {
        s_tray->setVisible(visible);
    }
}

#ifdef Q_OS_WIN
namespace
{
using Microsoft::WRL::ComPtr;
using Microsoft::WRL::Wrappers::HStringReference;
namespace Dom = ABI::Windows::Data::Xml::Dom;
namespace Notifications = ABI::Windows::UI::Notifications;

// Whether Kudari runs from its package, whose identity notifications of Windows need.
bool hasPackage()
{
    UINT32 length = 0;
    return GetCurrentPackageFullName(&length, nullptr) == ERROR_INSUFFICIENT_BUFFER;
}

// Puts `text` into the element `node`, as a node of text: the title of a session, which
// programs set, cannot bring markup of its own into the notification.
bool setText(Dom::IXmlDocument *document, Dom::IXmlNode *node, const QString &text)
{
    const std::wstring wide = text.toStdWString();
    ComPtr<Dom::IXmlText> content;
    ComPtr<Dom::IXmlNode> contentNode;
    ComPtr<Dom::IXmlNode> appended;
    return SUCCEEDED(document->CreateTextNode(HStringReference(wide.c_str(), UINT32(wide.size())).Get(), &content))
        && SUCCEEDED(content.As(&contentNode)) && SUCCEEDED(node->AppendChild(contentNode.Get(), &appended));
}

// A notification of Windows with `title` and `text`, in the group of `event`.
bool showToast(const QString &title, const QString &text, const QString &event)
{
    ComPtr<Notifications::IToastNotificationManagerStatics> manager;
    ComPtr<Dom::IXmlDocument> document;
    ComPtr<Dom::IXmlNodeList> texts;
    UINT32 count = 0;
    if (FAILED(RoGetActivationFactory(HStringReference(RuntimeClass_Windows_UI_Notifications_ToastNotificationManager).Get(),
                                      IID_PPV_ARGS(&manager)))
        || FAILED(manager->GetTemplateContent(Notifications::ToastTemplateType_ToastText02, &document))
        || FAILED(document->GetElementsByTagName(HStringReference(L"text").Get(), &texts))
        || FAILED(texts->get_Length(&count)) || count < 2) {
        return false;
    }
    ComPtr<Dom::IXmlNode> titleNode;
    ComPtr<Dom::IXmlNode> textNode;
    if (FAILED(texts->Item(0, &titleNode)) || FAILED(texts->Item(1, &textNode)) || !setText(document.Get(), titleNode.Get(), title)
        || !setText(document.Get(), textNode.Get(), text)) {
        return false;
    }
    ComPtr<Notifications::IToastNotificationFactory> factory;
    ComPtr<Notifications::IToastNotification> toast;
    if (FAILED(RoGetActivationFactory(HStringReference(RuntimeClass_Windows_UI_Notifications_ToastNotification).Get(),
                                      IID_PPV_ARGS(&factory)))
        || FAILED(factory->CreateToastNotification(document.Get(), &toast))) {
        return false;
    }
    ComPtr<Notifications::IToastNotification2> grouped;
    if (!event.isEmpty() && SUCCEEDED(toast.As(&grouped))) {
        const std::wstring group = event.toStdWString();
        grouped->put_Group(HStringReference(group.c_str(), UINT32(group.size())).Get());
    }
    ComPtr<Notifications::IToastNotifier> notifier;
    return SUCCEEDED(manager->CreateToastNotifier(&notifier)) && SUCCEEDED(notifier->Show(toast.Get()));
}
}
#endif

void rk_win_notify(const QString &title, const QString &text, const QString &event)
{
#ifdef Q_OS_WIN
    // With the package, Windows shows the notification itself and keeps it in its list, also
    // without the icon in the tray; the icon is what is left without it.
    if (hasPackage() && showToast(title, text, event)) {
        qCInfo(lcNotify).noquote() << "toast" << title << text;
        return;
    }
#else
    Q_UNUSED(event)
#endif
    if (!s_tray) {
        return;
    }
    // Windows shows the messages of a program through its icon in the tray. Without the
    // icon, it shows for the message, as long as a message stays on screen, and goes again.
    if (!s_trayWanted) {
        if (!s_trayHide) {
            s_trayHide = new QTimer(s_tray);
            s_trayHide->setSingleShot(true);
            s_trayHide->setInterval(10000);
            QObject::connect(s_trayHide, &QTimer::timeout, s_tray, [] {
                if (!s_trayWanted) {
                    s_tray->setVisible(false);
                }
            });
        }
        s_tray->setVisible(true);
        s_trayHide->start();
    }
    qCInfo(lcNotify).noquote() << "tray" << title << text;
    s_tray->showMessage(title, text, s_tray->icon());
}
