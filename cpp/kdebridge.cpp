// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

#define TRANSLATION_DOMAIN "kudari"

#include "kdebridge.h"
#include "kudari/src/kde.cxx.h"

#include <KAboutData>
#include <KActionCollection>
#include <KColorScheme>
#include <KConfig>
#include <KConfigGroup>
#include <KConfigWatcher>
#include <KCrash>
#include <KGlobalAccel>
#include <KGlobalShortcutInfo>
#include <KHelpMenu>
// kio/global.h uses one of its own deprecated types.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"
#include <KIO/CopyJob>
#include <KIO/JobUiDelegateFactory>
#include <KIO/OpenUrlJob>
#pragma GCC diagnostic pop
#include <KLocalizedQmlContext>
#include <KLocalizedString>
#include <KMessageBox>
#include <KNotification>
#include <KNotifyConfigWidget>
#include <KPluginMetaData>
#include <KSharedConfig>
#include <KShortcutsDialog>
#include <KStandardAction>
#include <KStandardActions>
#include <KStatusNotifierItem>
#include <KToggleFullScreenAction>
#include <KUriFilter>
#include <KWindowEffects>

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QCommandLineParser>
#include <QDBusConnection>
#include <QColorDialog>
#include <QInputDialog>
#include <QPainter>
#include <QPointer>
#include <QPushButton>
#include <QFontMetrics>
#include <QQmlApplicationEngine>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QIcon>
#include <QInputMethodEvent>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QStandardPaths>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeySequence>
#include <QMenu>
#include <QMimeData>
#include <QPrintDialog>
#include <QPrinter>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QRect>
#include <QTextDocument>
#include <QTimer>
#include <QUrl>
#include <QWindow>

#ifdef RK_TEST_HOOKS
#include <QTest>
#endif

namespace
{
KActionCollection *s_actions = nullptr;
KHelpMenu *s_helpMenu = nullptr;
KStatusNotifierItem *s_tray = nullptr;
QMenu *s_mainMenu = nullptr;
QMenu *s_screenMenu = nullptr;
QMenu *s_widthMenu = nullptr;
QMenu *s_heightMenu = nullptr;
QMenu *s_sessionMenu = nullptr;
QObject *s_owner = nullptr;
QJsonObject s_menuState;
// The open question whether an AI agent may use Kudari.
QPointer<QDialog> s_agentQuestion;

QQuickWindow *mainWindow()
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *window : windows) {
        if (auto quickWindow = qobject_cast<QQuickWindow *>(window)) {
            return quickWindow;
        }
    }
    return nullptr;
}

void report(const QString &name, bool checked)
{
    kde_action_triggered(name, checked);
}

QAction *addAction(const QString &name, const QString &text, const QString &icon, const QKeySequence &shortcut = {}, bool checkable = false)
{
    QAction *action = s_actions->addAction(name);
    action->setText(text);
    if (!icon.isEmpty()) {
        action->setIcon(QIcon::fromTheme(icon));
    }
    if (!shortcut.isEmpty()) {
        s_actions->setDefaultShortcut(action, shortcut);
    }
    action->setCheckable(checkable);
    QObject::connect(action, &QAction::triggered, s_owner, [name](bool checked) {
        report(name, checked);
    });
    return action;
}

void connectStandard(QAction *action)
{
    const QString name = action->objectName();
    QObject::connect(action, &QAction::triggered, s_owner, [name](bool checked) {
        report(name, checked);
    });
}

QAction *standard(KStandardActions::StandardAction id)
{
    return s_actions->action(KStandardActions::name(id));
}

void setupActions()
{
    s_actions = new KActionCollection(s_owner);

    auto fullScreen = new KToggleFullScreenAction(s_owner);
    s_actions->setDefaultShortcut(fullScreen, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_F11));
    s_actions->addAction(QStringLiteral("view-full-screen"), fullScreen);
    connectStandard(fullScreen);

    auto *quit = KStandardAction::quit(s_owner, [] {
        report(QStringLiteral("file_quit"), false);
    }, s_actions);
    s_actions->setDefaultShortcut(quit, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Q));
    KStandardAction::aboutApp(s_owner, [] {
        rk_about(QStringLiteral("application"));
    }, s_actions);
    KStandardAction::aboutKDE(s_owner, [] {
        rk_about(QStringLiteral("kde"));
    }, s_actions);
    KStandardAction::keyBindings(s_owner, [] {
        rk_configure_keys();
    }, s_actions);
    KStandardAction::configureNotifications(s_owner, [] {
        rk_configure_notifications();
    }, s_actions);
    KStandardAction::preferences(s_owner, [] {
        report(QStringLiteral("options_configure"), false);
    }, s_actions);

    // As in other terminals, plain Ctrl+C has to stay available for the shell.
    auto *copy = KStandardAction::copy(s_owner, [] {
        report(QStringLiteral("edit_copy"), false);
    }, s_actions);
    s_actions->setDefaultShortcut(copy, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_C));
    auto *paste = KStandardAction::paste(s_owner, [] {
        report(QStringLiteral("edit_paste"), false);
    }, s_actions);
    s_actions->setDefaultShortcuts(paste, {QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_V), QKeySequence(Qt::SHIFT | Qt::Key_Insert)});
    auto *selectAll = KStandardAction::selectAll(s_owner, [] {
        report(QStringLiteral("edit_select_all"), false);
    }, s_actions);
    s_actions->setDefaultShortcuts(selectAll, {});
    auto *find = KStandardAction::find(s_owner, [] {
        report(QStringLiteral("edit_find"), false);
    }, s_actions);
    s_actions->setDefaultShortcut(find, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_F));
    KStandardAction::findNext(s_owner, [] {
        report(QStringLiteral("edit_find_next"), false);
    }, s_actions);
    KStandardAction::findPrev(s_owner, [] {
        report(QStringLiteral("edit_find_prev"), false);
    }, s_actions);
    KStandardAction::saveAs(s_owner, [] {
        report(QStringLiteral("file_save_as"), false);
    }, s_actions);
    auto *print = KStandardAction::print(s_owner, [] {
        report(QStringLiteral("file_print"), false);
    }, s_actions);
    s_actions->setDefaultShortcut(print, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_P));

    QAction *toggle = addAction(QStringLiteral("toggle-window-state"), xi18nc("@action", "Show/Hide Kudari"), QStringLiteral("utilities-terminal"));
    KGlobalAccel::self()->setGlobalShortcut(toggle, QList<QKeySequence>() << QKeySequence(Qt::Key_F12));

    addAction(QStringLiteral("keep-open"), xi18nc("@action", "Keep Open When Focus Moves Elsewhere"), {}, {}, true);
    addAction(QStringLiteral("edit-profile"), xi18nc("@action", "Terminal Settings…"), QStringLiteral("document-properties"));

    addAction(QStringLiteral("increase-window-width"), xi18nc("@action", "Widen Window"), {}, QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_Right));
    addAction(QStringLiteral("decrease-window-width"), xi18nc("@action", "Narrow Window"), {}, QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_Left));
    addAction(QStringLiteral("increase-window-height"), xi18nc("@action", "Make Window Taller"), {}, QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_Down));
    addAction(QStringLiteral("decrease-window-height"), xi18nc("@action", "Make Window Shorter"), {}, QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_Up));

    addAction(QStringLiteral("new-session"), xi18nc("@action", "New Session"), QStringLiteral("tab-new"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_T));
    addAction(QStringLiteral("new-session-two-horizontal"), xi18nc("@action", "New Session, Side by Side"), QStringLiteral("tab-new"));
    addAction(QStringLiteral("new-session-two-vertical"), xi18nc("@action", "New Session, Stacked"), QStringLiteral("tab-new"));
    addAction(QStringLiteral("new-session-quad"), xi18nc("@action", "New Session, Four Terminals"), QStringLiteral("tab-new"));
    addAction(QStringLiteral("close-session"), xi18nc("@action", "Close Session"), QStringLiteral("tab-close"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_W));

    addAction(QStringLiteral("previous-session"), xi18nc("@action", "Previous Session"), QStringLiteral("go-previous"), QKeySequence(Qt::SHIFT | Qt::Key_Left));
    addAction(QStringLiteral("next-session"), xi18nc("@action", "Next Session"), QStringLiteral("go-next"), QKeySequence(Qt::SHIFT | Qt::Key_Right));
    addAction(QStringLiteral("move-session-left"), xi18nc("@action", "Move Session Left"), QStringLiteral("arrow-left"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Left));
    addAction(QStringLiteral("move-session-right"), xi18nc("@action", "Move Session Right"), QStringLiteral("arrow-right"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Right));

    addAction(QStringLiteral("grow-terminal-right"), xi18nc("@action", "Expand Terminal Right"), QStringLiteral("arrow-right"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_Right));
    addAction(QStringLiteral("grow-terminal-left"), xi18nc("@action", "Expand Terminal Left"), QStringLiteral("arrow-left"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_Left));
    addAction(QStringLiteral("grow-terminal-top"), xi18nc("@action", "Expand Terminal Up"), QStringLiteral("arrow-up"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_Up));
    addAction(QStringLiteral("grow-terminal-bottom"), xi18nc("@action", "Expand Terminal Down"), QStringLiteral("arrow-down"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_Down));

    addAction(QStringLiteral("rename-session"), xi18nc("@action", "Rename Session..."), QStringLiteral("edit-rename"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_S));
    addAction(QStringLiteral("previous-terminal"), xi18nc("@action", "Previous Terminal"), QStringLiteral("go-previous"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Tab));
    addAction(QStringLiteral("next-terminal"), xi18nc("@action", "Next Terminal"), QStringLiteral("go-next"), QKeySequence(Qt::CTRL | Qt::Key_Tab));
    addAction(QStringLiteral("close-active-terminal"), xi18nc("@action", "Close Terminal"), QStringLiteral("view-close"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_R));
    addAction(QStringLiteral("split-left-right"), xi18nc("@action", "Split into Columns"), QStringLiteral("view-split-left-right"), QKeySequence(Qt::CTRL | Qt::Key_ParenLeft));
    addAction(QStringLiteral("split-top-bottom"), xi18nc("@action", "Split into Rows"), QStringLiteral("view-split-top-bottom"), QKeySequence(Qt::CTRL | Qt::Key_ParenRight));
    addAction(QStringLiteral("split-auto"), xi18nc("@action", "Split Automatically"), QStringLiteral("view-split-auto"), QKeySequence(Qt::CTRL | Qt::Key_Asterisk));

    addAction(QStringLiteral("toggle-session-prevent-closing"), xi18nc("@action", "Protect from Closing"), {}, {}, true);
    addAction(QStringLiteral("toggle-session-keyboard-input"), xi18nc("@action", "Disable Keyboard Input"), {}, {}, true);
    addAction(QStringLiteral("toggle-session-monitor-activity"), xi18nc("@action", "Watch for Activity"), {}, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_A), true);
    addAction(QStringLiteral("toggle-session-monitor-silence"), xi18nc("@action", "Watch for Silence"), {}, QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_I), true);

    for (uint i = 1; i <= 10; ++i) {
        const QKeySequence shortcut = i < 10 ? QKeySequence(QStringLiteral("Alt+%1").arg(i)) : QKeySequence(Qt::ALT | Qt::Key_0);
        addAction(QStringLiteral("switch-to-session-%1").arg(i), xi18nc("@action", "Switch to Session %1", i), {}, shortcut);
    }

    addAction(QStringLiteral("clear-scrollback"), xi18nc("@action", "Clear Scrollback"), QStringLiteral("edit-clear-history"));

    addAction(QStringLiteral("clear-history-and-reset"), xi18nc("@action", "Clear Scrollback and Reset"), QStringLiteral("edit-clear-history"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_K));
    addAction(QStringLiteral("select-line"), xi18nc("@action", "Select Line"), QStringLiteral("edit-select"));
    addAction(QStringLiteral("select-mode"), xi18nc("@action", "Select Mode"), QStringLiteral("edit-select"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_D));
    addAction(QStringLiteral("paste-selection"), xi18nc("@action", "Paste Selection"), QStringLiteral("edit-paste"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Insert));
    addAction(QStringLiteral("open-browser"), xi18nc("@action", "Open File Manager"), QStringLiteral("system-file-manager"));
    addAction(QStringLiteral("web-search"), xi18nc("@action", "Search the Web"), QStringLiteral("preferences-web-browser-shortcuts"));
    addAction(QStringLiteral("auto-save"), xi18nc("@action", "Auto Save Output"), QStringLiteral("document-save"), {}, true);
    addAction(QStringLiteral("toggle-session-monitor-process"), xi18nc("@action", "Monitor for Process Finishing"), {}, {}, true);
    addAction(QStringLiteral("toggle-session-monitor-once"), xi18nc("@action", "One-Shot Monitors"), {}, {}, true);
    addAction(QStringLiteral("adjust-history"), xi18nc("@action", "Adjust Scrollback..."), QStringLiteral("configure"));
    addAction(QStringLiteral("allow-mouse-tracking"), xi18nc("@action", "Allow Mouse Tracking"), {}, {}, true);

    QAction *enlarge = addAction(QStringLiteral("enlarge-font"), xi18nc("@action", "Enlarge Font"), QStringLiteral("format-font-size-more"));
    s_actions->setDefaultShortcuts(enlarge, {QKeySequence(Qt::CTRL | Qt::Key_Plus), QKeySequence(Qt::CTRL | Qt::Key_Equal)});
    addAction(QStringLiteral("shrink-font"), xi18nc("@action", "Shrink Font"), QStringLiteral("format-font-size-less"), QKeySequence(Qt::CTRL | Qt::Key_Minus));
    addAction(QStringLiteral("reset-font-size"), xi18nc("@action", "Reset Font Size"), QStringLiteral("format-font-size-less"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_0));

    s_actions->readSettings();
}

void anchorToWindow(QMenu *menu)
{
    menu->winId();
    if (auto window = mainWindow(); window && menu->windowHandle()) {
        menu->windowHandle()->setTransientParent(window);
    }
}

void popup(QMenu *menu, int x, int y)
{
    auto window = mainWindow();
    anchorToWindow(menu);
    menu->popup(window ? window->mapToGlobal(QPoint(x, y)) : QPoint(x, y));
}

QMenu *percentMenu(const QString &title, const QString &kind)
{
    auto menu = new QMenu(title, s_mainMenu);
    for (int i = 10; i <= 100; i += 10) {
        QAction *action = menu->addAction(i18n("%1%", i));
        action->setCheckable(true);
        action->setData(i);
    }
    QObject::connect(menu, &QMenu::triggered, s_owner, [kind](QAction *action) {
        kde_menu_value(kind, action->data().toInt());
    });
    return menu;
}

void setupMainMenu()
{
    s_mainMenu = new QMenu();
    s_mainMenu->insertSection(nullptr, xi18nc("@title:menu", "Help"));
    s_mainMenu->addAction(standard(KStandardActions::AboutApp));
    s_mainMenu->addAction(standard(KStandardActions::AboutKDE));

    s_mainMenu->insertSection(nullptr, xi18nc("@title:menu", "Quick Settings"));
    s_mainMenu->addAction(s_actions->action(QStringLiteral("view-full-screen")));
    s_mainMenu->addAction(s_actions->action(QStringLiteral("keep-open")));

    s_screenMenu = new QMenu(xi18nc("@title:menu", "Screen"), s_mainMenu);
    QObject::connect(s_screenMenu, &QMenu::triggered, s_owner, [](QAction *action) {
        kde_menu_value(QStringLiteral("screen"), action->data().toInt());
    });
    s_mainMenu->addMenu(s_screenMenu);

    s_widthMenu = percentMenu(xi18nc("@title:menu", "Width"), QStringLiteral("width"));
    s_mainMenu->addMenu(s_widthMenu);
    s_heightMenu = percentMenu(xi18nc("@title:menu", "Height"), QStringLiteral("height"));
    s_mainMenu->addMenu(s_heightMenu);

    s_mainMenu->insertSection(nullptr, xi18nc("@title:menu", "Settings"));
    s_mainMenu->addAction(standard(KStandardActions::KeyBindings));
    s_mainMenu->addAction(standard(KStandardActions::ConfigureNotifications));
    s_mainMenu->addAction(standard(KStandardActions::Preferences));
    s_mainMenu->addSeparator();
    s_mainMenu->addAction(standard(KStandardActions::Quit));
}

void setupMenus()
{
    setupMainMenu();

    s_sessionMenu = new QMenu();
    s_sessionMenu->addAction(s_actions->action(QStringLiteral("new-session")));
    s_sessionMenu->addSeparator();
    s_sessionMenu->addAction(s_actions->action(QStringLiteral("new-session-two-horizontal")));
    s_sessionMenu->addAction(s_actions->action(QStringLiteral("new-session-two-vertical")));
    s_sessionMenu->addAction(s_actions->action(QStringLiteral("new-session-quad")));
}

void updateScreenMenu()
{
    s_screenMenu->clear();
    const int screen = s_menuState.value(QStringLiteral("screen")).toInt();
    const int count = s_menuState.value(QStringLiteral("screens")).toInt(1);

    QAction *action = s_screenMenu->addAction(xi18nc("@item:inmenu", "Where the mouse pointer is"));
    action->setCheckable(true);
    action->setData(0);
    action->setChecked(screen == 0);
    for (int i = 1; i <= count; ++i) {
        action = s_screenMenu->addAction(xi18nc("@item:inmenu", "Screen %1", i));
        action->setCheckable(true);
        action->setData(i);
        action->setChecked(i == screen);
    }
    s_screenMenu->menuAction()->setVisible(count > 1);
}

void updatePercentMenu(QMenu *menu, int value)
{
    const auto actions = menu->actions();
    for (QAction *action : actions) {
        action->setChecked(action->data().toInt() == value);
    }
}

void updateMenuState()
{
    updateScreenMenu();
    updatePercentMenu(s_widthMenu, s_menuState.value(QStringLiteral("width")).toInt());
    updatePercentMenu(s_heightMenu, s_menuState.value(QStringLiteral("height")).toInt());
}

/// Ports the per-terminal submenus of TabBar::updateToggle*Menu.
void addToggle(QMenu *menu, const QString &actionName, const QString &singleText, const QString &sessionText, const QString &menuTitle,
               const QString &terminalText, const QString &kind, const QJsonArray &terminals, const QString &stateKey)
{
    QAction *action = s_actions->action(actionName);
    if (terminals.size() <= 1) {
        action->setText(singleText);
        menu->addAction(action);
        return;
    }

    action->setText(sessionText);
    auto submenu = menu->addMenu(menuTitle);
    submenu->setAttribute(Qt::WA_DeleteOnClose);
    QObject::connect(submenu, &QMenu::hovered, s_owner, [](QAction *hovered) {
        kde_menu_hovered(hovered->data().isNull() ? -1 : hovered->data().toInt());
    });
    submenu->addAction(action);
    submenu->addSeparator();
    int count = 0;
    for (const auto &value : terminals) {
        const QJsonObject terminal = value.toObject();
        ++count;
        QAction *entry = submenu->addAction(terminalText.arg(count));
        entry->setCheckable(true);
        entry->setChecked(terminal.value(stateKey).toBool());
        const int id = terminal.value(QStringLiteral("id")).toInt();
        entry->setData(id);
        QObject::connect(entry, &QAction::triggered, s_owner, [kind, id](bool checked) {
            kde_terminal_toggled(kind, id, checked);
        });
    }
}
}

void rk_command_line(const QString &version)
{
    KLocalizedString::setApplicationDomain("kudari");
    QGuiApplication::setDesktopFileName(QStringLiteral("io.github.marcjose.kudari"));

    KAboutData about(QStringLiteral("kudari"),
                     QStringLiteral("Kudari"),
                     version,
                     xi18nc("@title", "A drop-down terminal for KDE Plasma on Wayland"),
                     KAboutLicense::MIT,
                     QStringLiteral("© 2026 Marc Jose"),
                     xi18nc("@info:credit", "Kudari started as a rewrite of Yakuake in Rust."),
                     QString(),
                     QString());
    about.setOrganizationDomain(QByteArray("marcjose.github.io"));
    about.setDesktopFileName(QStringLiteral("io.github.marcjose.kudari"));
    about.addCredit(QStringLiteral("Eike Hein and the Yakuake developers"),
                    xi18nc("@info:credit", "Yakuake, the drop-down terminal Kudari grew out of"),
                    QString(),
                    QStringLiteral("https://apps.kde.org/yakuake"));
    about.addCredit(QStringLiteral("The Konsole developers"),
                    xi18nc("@info:credit", "Ideas for many terminal options"),
                    QString(),
                    QStringLiteral("https://konsole.kde.org"));
    KAboutData::setApplicationData(about);

    QCommandLineParser parser;
    about.setupCommandLine(&parser);
    // main() handles it before Qt starts; it is named here for --help.
    parser.addPositionalArgument(QStringLiteral("mcp"),
                                 i18nc("@info:shell", "Serve the Model Context Protocol to an AI agent on standard input and output"),
                                 QStringLiteral("[mcp]"));
    parser.process(*qApp);
    about.processCommandLine(&parser);
}

// The icon of Kudari, or that of terminals while Kudari is not installed, as in a build.
static QString appIcon(bool symbolic)
{
    const QString own = symbolic ? QStringLiteral("io.github.marcjose.kudari-symbolic") : QStringLiteral("io.github.marcjose.kudari");
    if (QIcon::hasThemeIcon(own)) {
        return own;
    }
    return symbolic ? QStringLiteral("utilities-terminal-symbolic") : QStringLiteral("utilities-terminal");
}

void rk_init()
{
    KCrash::initialize();

    QGuiApplication::setWindowIcon(QIcon::fromTheme(appIcon(false)));
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE")) {
        QQuickStyle::setStyle(QStringLiteral("org.kde.desktop"));
    }

    s_owner = new QObject(qApp);
    for (const QString &file : {QStringLiteral("kdeglobals"), QStringLiteral("plasmarc")}) {
        static QList<KConfigWatcher::Ptr> watchers;
        watchers.append(KConfigWatcher::create(KSharedConfig::openConfig(file)));
        QObject::connect(watchers.last().data(), &KConfigWatcher::configChanged, s_owner, [] {
            kde_theme_changed();
        });
    }
    setupActions();
    setupMenus();
    s_helpMenu = new KHelpMenu(nullptr, KAboutData::applicationData());
}

void rk_finish_startup(bool showTray)
{
    rk_set_tray_visible(showTray);
}

QString rk_config_read(const QString &group, const QString &key, const QString &fallback)
{
    return KSharedConfig::openConfig()->group(group).readEntry(key, fallback);
}

void rk_config_write(const QString &group, const QString &key, const QString &value)
{
    KConfigGroup configGroup = KSharedConfig::openConfig()->group(group);
    configGroup.writeEntry(key, value);
    configGroup.sync();
}

QString rk_shortcuts_json()
{
    QJsonArray result;
    const auto actions = s_actions->actions();
    for (QAction *action : actions) {
        const auto shortcuts = action->shortcuts();
        if (shortcuts.isEmpty() || action->objectName() == QLatin1String("toggle-window-state")) {
            continue;
        }
        QJsonArray sequences;
        for (const QKeySequence &sequence : shortcuts) {
            sequences.append(sequence.toString(QKeySequence::PortableText));
        }
        result.append(QJsonObject{{QStringLiteral("name"), action->objectName()}, {QStringLiteral("sequences"), sequences}});
    }
    return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
}

QString rk_highlight_color()
{
    return KColorScheme(QPalette::Active, KColorScheme::View).decoration(KColorScheme::HoverColor).color().name();
}

QString rk_available_screen_rect(const QString &output)
{
    auto message = QDBusMessage::createMethodCall(QStringLiteral("org.kde.plasmashell"),
                                                  QStringLiteral("/StrutManager"),
                                                  QStringLiteral("org.kde.PlasmaShell.StrutManager"),
                                                  QStringLiteral("availableScreenRect"));
    message.setArguments({output});
    QDBusReply<QRect> reply = QDBusConnection::sessionBus().call(message, QDBus::Block, 500);
    if (!reply.isValid()) {
        return {};
    }
    const QRect rect = reply.value();
    return QStringLiteral("%1,%2,%3,%4").arg(rect.x()).arg(rect.y()).arg(rect.width()).arg(rect.height());
}

QString rk_active_output_name()
{
    auto message = QDBusMessage::createMethodCall(QStringLiteral("org.kde.KWin"), QStringLiteral("/KWin"), QStringLiteral("org.kde.KWin"), QStringLiteral("activeOutputName"));
    QDBusReply<QString> reply = QDBusConnection::sessionBus().call(message, QDBus::Block, 500);
    return reply.isValid() ? reply.value() : QString();
}

QString rk_output_names()
{
    QStringList names;
    const auto screens = QGuiApplication::screens();
    for (QScreen *screen : screens) {
        names << screen->name();
    }
    return names.join(QLatin1Char(','));
}

void rk_set_action_state(const QString &name, bool checked, bool enabled)
{
    if (QAction *action = s_actions->action(name)) {
        const bool blocked = action->blockSignals(true);
        if (action->isCheckable()) {
            action->setChecked(checked);
        }
        action->setEnabled(enabled);
        action->blockSignals(blocked);
    }
}

void rk_set_action_text(const QString &name, const QString &text)
{
    if (QAction *action = s_actions->action(name)) {
        action->setText(text);
    }
}

void rk_set_menu_state(const QString &json)
{
    s_menuState = QJsonDocument::fromJson(json.toUtf8()).object();
    updateMenuState();
    if (s_tray) {
        const auto shortcuts = KGlobalAccel::self()->shortcut(s_actions->action(QStringLiteral("toggle-window-state")));
        if (!shortcuts.isEmpty()) {
            s_tray->setToolTip(appIcon(true),
                               QStringLiteral("Kudari"),
                               xi18nc("@info", "<shortcut>%1</shortcut> shows the window", shortcuts.first().toString(QKeySequence::NativeText)));
        }
    }
}

void rk_set_tray_visible(bool visible)
{
    if (!visible) {
        if (s_tray) {
            // The tray item owns its context menu and deletes it with itself.
            delete s_tray;
            s_tray = nullptr;
            setupMainMenu();
            updateMenuState();
        }
        return;
    }
    if (s_tray) {
        return;
    }
    s_tray = new KStatusNotifierItem(s_owner);
    s_tray->setStandardActionsEnabled(false);
    s_tray->setIconByName(appIcon(true));
    s_tray->setStatus(KStatusNotifierItem::Active);
    s_tray->setContextMenu(s_mainMenu);
    s_tray->setAssociatedWindow(nullptr);
    QObject::connect(s_tray, &KStatusNotifierItem::activateRequested, s_owner, [] {
        report(QStringLiteral("toggle-window-state"), false);
    });
}

void rk_reload_shortcuts()
{
    s_actions->readSettings();
}

QString rk_toggle_shortcut()
{
    const auto shortcuts = KGlobalAccel::self()->shortcut(s_actions->action(QStringLiteral("toggle-window-state")));
    return shortcuts.isEmpty() ? QString() : shortcuts.first().toString(QKeySequence::PortableText);
}

QString rk_foreign_toggle_shortcut()
{
    const auto shortcuts = KGlobalAccel::self()->globalShortcut(QStringLiteral("yakuake"), QStringLiteral("toggle-window-state"));
    return shortcuts.isEmpty() ? QString() : shortcuts.first().toString(QKeySequence::NativeText);
}

void rk_set_toggle_shortcut(const QString &sequence)
{
    const QKeySequence key(sequence);
    // The daemon counts the key of Kudari's own action as taken as well, so only other owners are asked about.
    QList<KGlobalShortcutInfo> others = KGlobalAccel::globalShortcutsByKey(key);
    others.removeIf([](const KGlobalShortcutInfo &info) {
        return info.componentUniqueName() == QCoreApplication::applicationName();
    });
    if (!others.isEmpty()) {
        if (!KGlobalAccel::promptStealShortcutSystemwide(nullptr, others, key)) {
            return;
        }
        KGlobalAccel::stealShortcutSystemwide(key);
    }
    QAction *action = s_actions->action(QStringLiteral("toggle-window-state"));
    KGlobalAccel::self()->setShortcut(action, QList<QKeySequence>() << key, KGlobalAccel::NoAutoloading);
}

void rk_popup_main_menu(int x, int y)
{
    popup(s_mainMenu, x, y);
}

void rk_popup_session_menu(int x, int y)
{
    popup(s_sessionMenu, x, y);
}

void rk_popup_tab_menu(int x, int y, const QString &json)
{
    const QJsonObject state = QJsonDocument::fromJson(json.toUtf8()).object();
    const QJsonArray terminals = state.value(QStringLiteral("terminals")).toArray();

    auto menu = new QMenu();
    menu->setAttribute(Qt::WA_DeleteOnClose);
    QObject::connect(menu, &QMenu::hovered, s_owner, [](QAction *action) {
        kde_menu_hovered(action->data().isNull() ? -1 : action->data().toInt());
    });
    QObject::connect(menu, &QMenu::aboutToHide, s_owner, [] {
        QTimer::singleShot(0, qApp, [] {
            kde_menu_closed();
        });
    });

    menu->addAction(s_actions->action(QStringLiteral("split-left-right")));
    menu->addAction(s_actions->action(QStringLiteral("split-top-bottom")));
    menu->addSeparator();
    menu->addAction(s_actions->action(QStringLiteral("edit-profile")));
    menu->addAction(s_actions->action(QStringLiteral("rename-session")));
    const int session = state.value(QStringLiteral("session")).toInt();
    const QColor tabColor(state.value(QStringLiteral("color")).toString());
    QAction *chooseColor = menu->addAction(QIcon::fromTheme(QStringLiteral("color-management")), xi18nc("@action", "Tab Color..."));
    QObject::connect(chooseColor, &QAction::triggered, s_owner, [session, tabColor] {
        // The menu is closing; the dialog must not run inside its signal.
        QTimer::singleShot(0, qApp, [session, tabColor] {
            kde_dialog_active(true);
            const QColor color = QColorDialog::getColor(tabColor);
            kde_dialog_active(false);
            if (color.isValid()) {
                kde_tab_color(session, int(color.rgb() & 0xffffff));
            }
        });
    });
    if (tabColor.isValid()) {
        QAction *resetColor = menu->addAction(xi18nc("@action", "Reset Tab Color"));
        QObject::connect(resetColor, &QAction::triggered, s_owner, [session] {
            kde_tab_color(session, -1);
        });
    }
    menu->addAction(s_actions->action(QStringLiteral("toggle-session-prevent-closing")));
    addToggle(menu,
              QStringLiteral("toggle-session-keyboard-input"),
              xi18nc("@action", "Disable Keyboard Input"),
              xi18nc("@action", "Whole Session"),
              xi18nc("@title:menu", "Disable Keyboard Input"),
              xi18nc("@action", "Terminal %1", QStringLiteral("%1")),
              QStringLiteral("keyboard"),
              terminals,
              QStringLiteral("keyboardDisabled"));
    addToggle(menu,
              QStringLiteral("toggle-session-monitor-activity"),
              xi18nc("@action", "Watch for Activity"),
              xi18nc("@action", "Whole Session"),
              xi18nc("@title:menu", "Watch for Activity"),
              xi18nc("@action", "Terminal %1", QStringLiteral("%1")),
              QStringLiteral("activity"),
              terminals,
              QStringLiteral("activity"));
    addToggle(menu,
              QStringLiteral("toggle-session-monitor-silence"),
              xi18nc("@action", "Watch for Silence"),
              xi18nc("@action", "Whole Session"),
              xi18nc("@title:menu", "Watch for Silence"),
              xi18nc("@action", "Terminal %1", QStringLiteral("%1")),
              QStringLiteral("silence"),
              terminals,
              QStringLiteral("silence"));
    menu->addAction(s_actions->action(QStringLiteral("toggle-session-monitor-process")));
    menu->addAction(s_actions->action(QStringLiteral("toggle-session-monitor-once")));
    menu->addSeparator();
    menu->addAction(s_actions->action(QStringLiteral("move-session-left")));
    menu->addAction(s_actions->action(QStringLiteral("move-session-right")));
    menu->addSeparator();
    menu->addAction(s_actions->action(QStringLiteral("close-active-terminal")));
    menu->addAction(s_actions->action(QStringLiteral("close-session")));

    popup(menu, x, y);
}

void rk_popup_terminal_menu(int x, int y, bool canCopy, const QString &link, bool autoSaving)
{
    auto menu = new QMenu();
    menu->setAttribute(Qt::WA_DeleteOnClose);
    if (!link.isEmpty()) {
        QAction *open = menu->addAction(QIcon::fromTheme(QStringLiteral("internet-services")), xi18nc("@action:inmenu", "Open Link"));
        QObject::connect(open, &QAction::triggered, s_owner, [link] {
            kde_open_link(link);
        });
        QAction *copyLink = menu->addAction(QIcon::fromTheme(QStringLiteral("edit-copy-path")), xi18nc("@action:inmenu", "Copy Link Address"));
        QObject::connect(copyLink, &QAction::triggered, s_owner, [link] {
            QGuiApplication::clipboard()->setText(link);
        });
        menu->addSeparator();
    }
    const auto add = [menu](const char *name) {
        menu->addAction(s_actions->action(QString::fromLatin1(name)));
    };
    add("split-left-right");
    add("split-top-bottom");
    menu->addSeparator();
    QAction *copy = standard(KStandardActions::Copy);
    copy->setEnabled(canCopy);
    menu->addAction(copy);
    menu->addAction(standard(KStandardActions::Paste));
    add("paste-selection");
    menu->addAction(standard(KStandardActions::SelectAll));
    add("select-line");
    add("select-mode");
    menu->addSeparator();
    menu->addAction(standard(KStandardActions::Find));
    QAction *webSearch = s_actions->action(QStringLiteral("web-search"));
    webSearch->setEnabled(canCopy);
    menu->addAction(webSearch);
    add("open-browser");
    menu->addSeparator();
    add("adjust-history");
    add("clear-scrollback");
    add("clear-history-and-reset");
    menu->addAction(standard(KStandardActions::SaveAs));
    menu->addAction(standard(KStandardActions::Print));
    rk_set_action_state(QStringLiteral("auto-save"), autoSaving, true);
    add("auto-save");
    add("allow-mouse-tracking");
    menu->addSeparator();

    auto signals_ = menu->addMenu(xi18nc("@title:menu", "Send Signal"));
    const struct {
        QString text;
        int number;
    } entries[] = {
        {i18nc("@item:inmenu", "Suspend Task (STOP)"), 19},
        {i18nc("@item:inmenu", "Continue Task (CONT)"), 18},
        {i18nc("@item:inmenu", "Hangup (HUP)"), 1},
        {i18nc("@item:inmenu", "Interrupt Task (INT)"), 2},
        {i18nc("@item:inmenu", "Terminate Task (TERM)"), 15},
        {i18nc("@item:inmenu", "Kill Task (KILL)"), 9},
        {i18nc("@item:inmenu", "User Signal 1 (USR1)"), 10},
        {i18nc("@item:inmenu", "User Signal 2 (USR2)"), 12},
    };
    for (const auto &entry : entries) {
        const int number = entry.number;
        QAction *action = signals_->addAction(entry.text);
        QObject::connect(action, &QAction::triggered, s_owner, [number] {
            kde_menu_value(QStringLiteral("signal"), number);
        });
    }
    popup(menu, x, y);
}

void rk_web_search(const QString &text)
{
    KUriFilterData data(text);
    data.setCheckForExecutables(false);
    if (KUriFilter::self()->filterSearchUri(data, KUriFilter::NormalTextFilter)) {
        rk_open_url(data.uri().toString());
    }
}

void rk_about(const QString &what)
{
    kde_dialog_active(true);
    if (what == QLatin1String("application")) {
        s_helpMenu->aboutApplication();
    } else {
        s_helpMenu->aboutKDE();
    }
    kde_dialog_active(false);
}

void rk_configure_keys()
{
    auto *dialog = new KShortcutsDialog(KShortcutsEditor::AllActions, KShortcutsEditor::LetterShortcutsAllowed);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    // A modal dialog would run its own event loop in configure().
    dialog->setModal(false);
    dialog->addCollection(s_actions);
    QObject::connect(dialog, &KShortcutsDialog::saved, [] {
        kde_shortcuts_changed();
    });
    QObject::connect(dialog, &QDialog::finished, [] {
        kde_dialog_active(false);
    });
    kde_dialog_active(true);
    dialog->configure();
}

void rk_configure_notifications()
{
    kde_dialog_active(true);
    KNotifyConfigWidget::configure(nullptr);
    kde_dialog_active(false);
}

void rk_notify(const QString &event, const QString &text)
{
    auto notification = new KNotification(event, KNotification::CloseWhenWindowActivated);
    notification->setWindow(mainWindow());
    notification->setText(text);
    notification->sendEvent();
}

void rk_ask_agent(const QString &agent, const QString &title, const QString &text, const QString &allow, const QString &deny)
{
    if (s_agentQuestion) {
        return;
    }
    auto *dialog = new QDialog;
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(title);
    dialog->setProperty("question", text);
    auto *buttons = new QDialogButtonBox(dialog);
    buttons->setStandardButtons(QDialogButtonBox::Yes | QDialogButtonBox::No);
    KGuiItem::assign(buttons->button(QDialogButtonBox::Yes), KGuiItem(allow, QStringLiteral("dialog-ok")));
    KGuiItem::assign(buttons->button(QDialogButtonBox::No), KGuiItem(deny, QStringLiteral("dialog-cancel")));
    KMessageBox::createKMessageBox(dialog, buttons, QMessageBox::Warning, text, {}, QString(), nullptr, KMessageBox::Options(KMessageBox::Notify | KMessageBox::Dangerous | KMessageBox::NoExec));
    // Return, Escape and closing the window all deny: only a deliberate click allows.
    buttons->button(QDialogButtonBox::No)->setDefault(true);
    buttons->button(QDialogButtonBox::No)->setFocus();
    s_agentQuestion = dialog;
    QObject::connect(dialog, &QDialog::finished, [agent](int result) {
        s_agentQuestion = nullptr;
        kde_dialog_active(false);
        kde_agent_answered(agent, result == QDialogButtonBox::Yes);
    });
    kde_dialog_active(true);
    dialog->show();
}

void rk_ask_question(int id, const QString &text, const QString &title, int continueKind)
{
    auto *dialog = new QDialog;
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(title);
    auto *buttons = new QDialogButtonBox(dialog);
    buttons->setStandardButtons(QDialogButtonBox::Yes | QDialogButtonBox::No);
    const KGuiItem continueItem = continueKind == 0 ? KStandardGuiItem::quit() : continueKind == 1 ? KStandardGuiItem::close() : KStandardGuiItem::cont();
    KGuiItem::assign(buttons->button(QDialogButtonBox::Yes), continueItem);
    KGuiItem::assign(buttons->button(QDialogButtonBox::No), KStandardGuiItem::cancel());
    KMessageBox::createKMessageBox(dialog, buttons, QMessageBox::Warning, text, {}, QString(), nullptr, KMessageBox::Options(KMessageBox::Notify | KMessageBox::NoExec));
    buttons->button(QDialogButtonBox::Yes)->setDefault(true);
    QObject::connect(dialog, &QDialog::finished, [id](int result) {
        kde_dialog_active(false);
        kde_question_answered(id, result == QDialogButtonBox::Yes);
    });
    kde_dialog_active(true);
    dialog->show();
}

void rk_close_agent_question()
{
    if (s_agentQuestion) {
        s_agentQuestion->reject();
    }
}

void rk_slide_window()
{
    if (auto window = mainWindow(); window && KWindowEffects::isEffectAvailable(KWindowEffects::Slide)) {
        KWindowEffects::slideWindow(window, KWindowEffects::TopEdge);
    }
}

#ifdef RK_TEST_HOOKS
void rk_test_key(const QString &sequence)
{
    const QKeyCombination combination = QKeySequence(sequence)[0];
    QTest::keyClick(mainWindow(), combination.key(), combination.keyboardModifiers());
}

void rk_test_input_method(const QString &preedit, const QString &commit)
{
    QInputMethodEvent event(preedit, {});
    if (!commit.isEmpty()) {
        event.setCommitString(commit);
    }
    QCoreApplication::sendEvent(QGuiApplication::focusObject(), &event);
}

QString rk_test_agent_question()
{
    return s_agentQuestion ? s_agentQuestion->property("question").toString() : QString();
}

void rk_test_answer_agent(bool allow)
{
    if (s_agentQuestion) {
        s_agentQuestion->findChild<QDialogButtonBox *>()->button(allow ? QDialogButtonBox::Yes : QDialogButtonBox::No)->click();
    }
}

// Every visible window of Qt Quick, the settings as well, named by where it is on the screen.
void rk_test_grab(const QString &path)
{
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *window : windows) {
        auto quick = qobject_cast<QQuickWindow *>(window);
        if (quick && quick->isVisible()) {
            const QPoint at = quick->position();
            quick->grabWindow().save(QStringLiteral("%1-%2-%3.png").arg(path).arg(at.x()).arg(at.y()));
        }
    }
}

QString rk_test_window_state()
{
    auto window = mainWindow();
    return QStringLiteral("visible=%1 active=%2 height=%3").arg(window->isVisible()).arg(window->isActive()).arg(window->height());
}
#else
void rk_test_key(const QString &)
{
}

void rk_test_input_method(const QString &, const QString &)
{
}

QString rk_test_agent_question()
{
    return {};
}

void rk_test_answer_agent(bool)
{
}

QString rk_test_window_state()
{
    return {};
}

void rk_test_grab(const QString &)
{
}
#endif

QString rk_tr(const QString &context, const QString &text, const QString &argument)
{
    KLocalizedString message = context.isEmpty() ? ki18nd("kudari", text.toUtf8().constData())
                                                 : kxi18ndc("kudari", context.toUtf8().constData(), text.toUtf8().constData());
    return text.contains(QLatin1String("%1")) ? message.subs(argument).toString() : message.toString();
}

QString rk_action_text(const QString &name)
{
    QAction *action = s_actions->action(name);
    return action ? KLocalizedString::removeAcceleratorMarker(action->text()) : QString();
}

static QString locateData(const QString &path)
{
    return QStandardPaths::locate(QStandardPaths::GenericDataLocation, path);
}

// Styles without graphics only recolor Breeze's desktop, as Breeze Twilight's does. A
// drop-down terminal belongs to the desktop, so they win over the application colors.
// Air and Oxygen have graphics: their colors were made for those, not for the applications.
static KSharedConfigPtr themeConfig(const QString &theme)
{
    QString style;
    QString scheme;
    if (theme.isEmpty()) {
        style = KConfig(QStringLiteral("plasmarc")).group(QStringLiteral("Theme")).readEntry("name", QStringLiteral("default"));
    } else if (!theme.contains(QLatin1Char('/'))) {
        const KConfig defaults(locateData(QStringLiteral("plasma/look-and-feel/%1/contents/defaults").arg(theme)), KConfig::SimpleConfig);
        style = defaults.group(QStringLiteral("plasmarc")).group(QStringLiteral("Theme")).readEntry("name", QStringLiteral("default"));
        scheme = defaults.group(QStringLiteral("kdeglobals")).group(QStringLiteral("General")).readEntry("ColorScheme");
    }
    const QString styleWidgets = QStringLiteral("plasma/desktoptheme/%1/widgets").arg(style);
    const bool hasGraphics = !QStandardPaths::locate(QStandardPaths::GenericDataLocation, styleWidgets, QStandardPaths::LocateDirectory).isEmpty();
    QString file = hasGraphics ? QString() : locateData(QStringLiteral("plasma/desktoptheme/%1/colors").arg(style));
    if (file.isEmpty() && !scheme.isEmpty()) {
        file = locateData(QStringLiteral("color-schemes/%1.colors").arg(scheme));
    }
    KSharedConfigPtr config = file.isEmpty() ? KSharedConfig::openConfig(QStringLiteral("kdeglobals")) : KSharedConfig::openConfig(file, KConfig::SimpleConfig);
    config->reparseConfiguration();
    return config;
}

QString rk_theme_colors(const QString &theme)
{
    const KSharedConfigPtr config = themeConfig(theme);
    const KColorScheme header(QPalette::Active, KColorScheme::Header, config);
    const KColorScheme window(QPalette::Active, KColorScheme::Window, config);
    const KColorScheme selection(QPalette::Active, KColorScheme::Selection, config);
    const KColorScheme view(QPalette::Active, KColorScheme::View, config);
    const QStringList colors = {
        header.background().color().name(),
        header.foreground().color().name(),
        window.background().color().name(),
        window.foreground().color().name(),
        selection.background().color().name(),
        selection.foreground().color().name(),
        window.foreground(KColorScheme::NegativeText).color().name(),
        view.background().color().name(),
        view.foreground().color().name(),
        view.foreground(KColorScheme::PositiveText).color().name(),
        view.foreground(KColorScheme::NeutralText).color().name(),
        view.foreground(KColorScheme::LinkText).color().name(),
        view.foreground(KColorScheme::VisitedText).color().name(),
        view.foreground(KColorScheme::ActiveText).color().name(),
        view.foreground(KColorScheme::InactiveText).color().name(),
    };
    return colors.join(QLatin1Char(','));
}

QString rk_global_themes_json()
{
    QMap<QString, QJsonObject> themes;
    const QStringList roots = QStandardPaths::locateAll(QStandardPaths::GenericDataLocation, QStringLiteral("plasma/look-and-feel"), QStandardPaths::LocateDirectory);
    for (const QString &root : roots) {
        const QStringList entries = QDir(root).entryList(QDir::Dirs | QDir::NoDotAndDotDot);
        for (const QString &id : entries) {
            if (!QFile::exists(QStringLiteral("%1/%2/contents/defaults").arg(root, id))) {
                continue;
            }
            const QString name = KPluginMetaData::fromJsonFile(QStringLiteral("%1/%2/metadata.json").arg(root, id)).name();
            if (!name.isEmpty()) {
                themes.insert(name, QJsonObject{{QStringLiteral("id"), id}, {QStringLiteral("name"), name}});
            }
        }
    }
    QJsonArray list;
    for (const QJsonObject &theme : std::as_const(themes)) {
        list.append(theme);
    }
    return QString::fromUtf8(QJsonDocument(list).toJson(QJsonDocument::Compact));
}

void rk_popup_drop_menu(int x, int y, int terminal, const QString &paths, const QString &directory)
{
    const QStringList files = paths.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    QList<QUrl> urls;
    for (const QString &file : files) {
        urls.append(QUrl::fromLocalFile(file));
    }
    auto menu = new QMenu();
    menu->setAttribute(Qt::WA_DeleteOnClose);
    QObject::connect(menu->addAction(QIcon::fromTheme(QStringLiteral("edit-paste")), xi18nc("@action:inmenu", "Paste Location")),
                     &QAction::triggered,
                     s_owner,
                     [terminal, paths] {
                         kde_terminal_paths(terminal, paths, false);
                     });
    if (files.size() == 1 && QFileInfo(files.first()).isDir()) {
        QObject::connect(menu->addAction(QIcon::fromTheme(QStringLiteral("folder")), xi18nc("@action:inmenu", "Change Directory To")),
                         &QAction::triggered,
                         s_owner,
                         [terminal, paths] {
                             kde_terminal_paths(terminal, paths, true);
                         });
    }
    if (!directory.isEmpty()) {
        menu->addSeparator();
        const QUrl target = QUrl::fromLocalFile(directory);
        const auto start = [](KIO::Job *job) {
            job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
        };
        QObject::connect(menu->addAction(QIcon::fromTheme(QStringLiteral("edit-copy")), xi18nc("@action:inmenu", "Copy Here")),
                         &QAction::triggered,
                         s_owner,
                         [urls, target, start] {
                             start(KIO::copy(urls, target));
                         });
        QObject::connect(menu->addAction(QIcon::fromTheme(QStringLiteral("go-jump")), xi18nc("@action:inmenu", "Move Here")),
                         &QAction::triggered,
                         s_owner,
                         [urls, target, start] {
                             start(KIO::move(urls, target));
                         });
        QObject::connect(menu->addAction(QIcon::fromTheme(QStringLiteral("edit-link")), xi18nc("@action:inmenu", "Link Here")),
                         &QAction::triggered,
                         s_owner,
                         [urls, target, start] {
                             start(KIO::link(urls, target));
                         });
    }
    menu->addSeparator();
    menu->addAction(QIcon::fromTheme(QStringLiteral("dialog-cancel")), xi18nc("@action:inmenu", "Cancel"));
    popup(menu, x, y);
}

void rk_setup_qml_i18n(QQmlApplicationEngine &engine)
{
    KLocalization::setupLocalizedContext(&engine);
}

void rk_open_url(const QString &url)
{
    auto job = new KIO::OpenUrlJob(url.startsWith(QLatin1Char('/')) ? QUrl::fromLocalFile(url) : QUrl(url));
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    job->setRunExecutables(false);
    job->start();
}

void rk_set_blur(bool enabled)
{
    if (auto window = mainWindow()) {
        KWindowEffects::enableBlurBehind(window, enabled);
    }
}
