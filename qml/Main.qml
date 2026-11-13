// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.settings as KirigamiSettings
import org.kde.layershell as LayerShell
import io.github.marcjose.kudari
import io.github.marcjose.kudari.qt

Window {
    id: root

    readonly property var skin: JSON.parse(app.skinJson)
    readonly property var settings: JSON.parse(app.settingsJson)
    readonly property var fonts: JSON.parse(app.fontsJson)
    readonly property var tabs: app.tabsJson.length > 0 ? JSON.parse(app.tabsJson) : []
    readonly property var layoutData: app.layoutJson.length > 0 ? JSON.parse(app.layoutJson) : ({terminals: [], handles: [], active: -1})
    readonly property var shortcuts: JSON.parse(app.shortcutsJson)

    readonly property int borderWidth: settings.hideSkinBorders ? 0 : 2
    // The bar at the bottom: the tabs with the window buttons, or only the buttons.
    readonly property int footerHeight: settings.showTabBar ? tabBar.height : buttonStrip.height
    // A slim grip below it changes the height of the window.
    readonly property int gripHeight: Kirigami.Units.smallSpacing

    // The window takes its share of the work area, and its position spreads the space left over.
    readonly property var targetScreen: Qt.application.screens.find(candidate => candidate.name === app.outputName) ?? Qt.application.screens[0]
    readonly property int workWidth: !app.fullscreen && app.workWidth > 0 ? app.workWidth : targetScreen.width
    readonly property int workHeight: !app.fullscreen && app.workHeight > 0 ? app.workHeight : targetScreen.height
    readonly property int windowWidth: app.fullscreen ? workWidth : Math.floor(workWidth * settings.width / 100)
    readonly property int windowHeight: app.fullscreen ? workHeight : Math.floor(workHeight * settings.height / 100)
    readonly property int windowX: app.fullscreen ? 0 : Math.floor(workWidth * settings.position * (100 - settings.width) / 10000)

    readonly property int contentBottom: windowHeight - gripHeight
    readonly property int stackHeight: contentBottom - footerHeight - borderWidth
    readonly property int stackWidth: windowWidth - 2 * borderWidth

    width: windowWidth
    height: windowHeight
    color: "transparent"
    title: app.title.length > 0 ? app.title : "Kudari"
    visible: app.windowVisible
    flags: Qt.FramelessWindowHint
    screen: targetScreen

    LayerShell.Window.scope: "kudari"
    LayerShell.Window.layer: app.dialogOpen ? LayerShell.Window.LayerBottom : LayerShell.Window.LayerTop
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop | LayerShell.Window.AnchorLeft
    LayerShell.Window.margins.left: windowX
    LayerShell.Window.exclusionZone: 0
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityOnDemand
    LayerShell.Window.wantsToBeOnActiveScreen: false

    onVisibleChanged: {
        if (visible) {
            app.afterShow();
        }
    }
    // F3 only belongs to the search while it is open; programs such as `mc` use it, too.
    property bool searchActive: false

    // True while the grip is being dragged. Terminals then only learn their new size on
    // release; every resize makes shells redraw their prompt.
    property bool gripDragged: false

    // A menu or dialog of Kudari takes the window focus; the cursor stays solid meanwhile.
    property bool applicationFocused: false

    onActiveChanged: {
        applicationFocused = active;
        if (!active) {
            Qt.callLater(root.handleFocusLoss);
        }
    }

    function handleFocusLoss() {
        applicationFocused = active || app.applicationHasFocus();
        if (!active) {
            app.focusLost();
        }
    }

    Controller {
        id: app

        onTerminalDirty: terminalId => {
            const pane = sessionArea.panes[terminalId];
            if (pane) {
                pane.item.refresh();
            }
        }
        onFocusTerminal: terminalId => focusLater.request(terminalId)
        onRenameRequested: session => tabBar.startRename(session)
        onTerminalIdsJsonChanged: sessionArea.update()
        onHighlightTerminal: (terminalId, persistent) => sessionArea.highlight(terminalId, persistent, app.highlightColor, root.settings.highlightOpacity, root.settings.highlightDuration)
        onRemoveHighlight: sessionArea.clearHighlight()
        onKeyboardBlocked: terminalId => sessionArea.highlight(terminalId, false, root.settings.blockColor, root.settings.blockOpacity, root.settings.blockDuration)
        onTerminalAction: (terminalId, action) => {
            const pane = sessionArea.panes[terminalId];
            if (!pane) {
                return;
            }
            if (action === "edit_copy") {
                pane.item.copySelection();
            } else if (action === "edit_paste") {
                // Pasting can ask for confirmation, which must not run inside this signal.
                Qt.callLater(pane.item.paste, false);
            } else if (action === "edit_find") {
                pane.openSearch();
            } else if (action === "edit_find_next") {
                pane.findNext(false);
            } else if (action === "edit_find_prev") {
                pane.findNext(true);
            } else if (action === "paste-selection") {
                Qt.callLater(pane.item.paste, true);
            } else if (action === "select-line") {
                pane.item.selectLine();
            } else if (action === "select-mode") {
                pane.item.toggleSelectionMode();
            } else if (action === "clear-scrollback") {
                pane.item.clearScrollback();
            } else {
                pane.item.selectAll();
            }
        }
        onQuitRequested: Qt.quit()
        onSettingsRequested: module => configuration.open(module)
        onFirstRunRequested: {
            app.dialogOpen = true;
            firstRun.active = true;
        }
        onDialogOpenChanged: {
            if (!dialogOpen && root.visible) {
                root.requestActivate();
            }
        }
    }

    Loader {
        id: firstRun

        active: false
        sourceComponent: FirstRunDialog {
            controller: app
            onFinished: {
                app.dialogOpen = false;
                firstRun.active = false;
            }
        }
    }

    KirigamiSettings.ConfigurationView {
        id: configuration

        title: app.actionText("options_configure").replace(/\s*(…|\.\.\.)$/, "")
        onConfigViewItemChanged: app.dialogOpen = configViewItem !== null
        modules: [
            KirigamiSettings.ConfigurationModule {
                moduleId: "window"
                text: app.translate("@title Preferences page name", "Window")
                icon.name: "preferences-system-windows-move"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "WindowSettingsPage")
                initialProperties: () => ({controller: app})
            },
            KirigamiSettings.ConfigurationModule {
                moduleId: "behavior"
                text: app.translate("@title Preferences page name", "Behavior")
                icon.name: "preferences-system-windows-actions"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "BehaviorSettingsPage")
                initialProperties: () => ({controller: app})
            },
            KirigamiSettings.ConfigurationModule {
                moduleId: "appearance"
                text: app.translate("@title Preferences page name", "Appearance")
                icon.name: "preferences-desktop-theme"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "AppearanceSettingsPage")
                initialProperties: () => ({controller: app})
            },
            KirigamiSettings.ConfigurationModule {
                moduleId: "terminal"
                text: app.translate("@title Preferences page name", "Terminal")
                icon.name: "utilities-terminal"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "TerminalSettingsPage")
                initialProperties: () => ({controller: app})
            },
            KirigamiSettings.ConfigurationModule {
                moduleId: "terminal-behavior"
                text: app.translate("@title Preferences page name", "Terminal Behavior")
                icon.name: "preferences-desktop-mouse"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "TerminalBehaviorPage")
                initialProperties: () => ({controller: app})
            },
            KirigamiSettings.ConfigurationModule {
                moduleId: "security"
                text: app.translate("@title Preferences page name", "Security")
                icon.name: "security-high"
                page: () => Qt.createComponent("io.github.marcjose.kudari.qt", "SecuritySettingsPage")
                initialProperties: () => ({controller: app})
            }
        ]
    }

    Timer {
        id: focusLater

        property int terminalId: -1

        function request(id) {
            terminalId = id;
            restart();
        }

        interval: 0
        onTriggered: {
            const pane = sessionArea.panes[terminalId];
            if (pane) {
                pane.focusTerminal();
            }
        }
    }

    Timer {
        interval: 500
        running: true
        repeat: true
        onTriggered: app.poll()
    }

    FontMetrics {
        id: terminalMetrics

        font: {
            const config = root.settings.config;
            const system = Kirigami.Theme.fixedWidthFont;
            return Qt.font({
                family: config.FontFamily.length > 0 ? config.FontFamily : system.family,
                pointSize: config.FontSize > 0 ? config.FontSize : system.pointSize
            });
        }
        onFontChanged: root.reportGeometry()
    }

    // Text sits on a grid of whole pixels. Unlike FontMetrics.advanceWidth(),
    // this property updates when the font changes.
    TextMetrics {
        id: cellMetrics

        font: terminalMetrics.font
        text: "M"
        onAdvanceWidthChanged: root.reportGeometry()
    }

    // The background behind the terminals, with the configured color and translucency.
    Rectangle {
        y: 0
        width: root.windowWidth
        height: root.contentBottom
        color: root.settings.backgroundColor
        opacity: root.settings.config.Opacity < 100 ? 0 : root.settings.translucency ? root.settings.backgroundOpacity / 100 : 1
    }

    Rectangle {
        visible: root.borderWidth > 0
        width: root.borderWidth
        height: root.contentBottom
        color: root.skin.borderColor
    }

    Rectangle {
        visible: root.borderWidth > 0
        x: root.windowWidth - root.borderWidth
        width: root.borderWidth
        height: root.contentBottom
        color: root.skin.borderColor
    }

    Rectangle {
        visible: root.borderWidth > 0
        y: root.contentBottom - root.borderWidth
        width: root.windowWidth
        height: root.borderWidth
        color: root.skin.borderColor
    }

    SessionArea {
        id: sessionArea

        x: root.borderWidth
        width: root.stackWidth
        height: root.stackHeight
        controller: app
        layoutData: root.layoutData
        settings: root.settings
        baseFont: terminalMetrics.font
        applicationFocused: root.applicationFocused
        resizePaused: root.gripDragged
        // The terminals, their search bars and size hints have the colors of the window.
        Kirigami.Theme.colorSet: Kirigami.Theme.Window
        Kirigami.Theme.inherit: false
        paneTheme.gridUnit: Kirigami.Units.gridUnit
        paneTheme.smallSpacing: Kirigami.Units.smallSpacing
        paneTheme.largeSpacing: Kirigami.Units.largeSpacing
        paneTheme.cornerRadius: Kirigami.Units.cornerRadius
        paneTheme.highlightColor: sessionArea.Kirigami.Theme.highlightColor
        paneTheme.backgroundColor: sessionArea.Kirigami.Theme.backgroundColor
        paneTheme.textColor: sessionArea.Kirigami.Theme.textColor
        paneTheme.negativeTextColor: sessionArea.Kirigami.Theme.negativeTextColor
        // Breeze draws splitters as a mix of window and text color.
        splitterColor: Kirigami.ColorUtils.linearInterpolation(sessionArea.Kirigami.Theme.backgroundColor, sessionArea.Kirigami.Theme.textColor, 0.2)

        onWidthChanged: root.reportGeometry()
        onHeightChanged: root.reportGeometry()
        onSearchOpenChanged: open => root.searchActive = open
    }

    function reportGeometry() {
        if (sessionArea.width > 0 && sessionArea.height > 0 && terminalMetrics.height > 0) {
            app.setGeometry(sessionArea.width, sessionArea.height, Math.round(cellMetrics.advanceWidth), Math.round(terminalMetrics.height) + settings.config.LineSpacing);
        }
    }

    TabBar {
        id: tabBar

        controller: app
        fonts: root.fonts
        tabs: root.tabs
        activityColor: root.settings.config.TabActivityColor
        keepOpen: root.settings.keepOpen
        visible: root.settings.showTabBar
        x: root.borderWidth
        y: root.contentBottom - height - root.borderWidth
        width: root.windowWidth - 2 * root.borderWidth
    }

    Rectangle {
        id: buttonStrip

        visible: !root.settings.showTabBar
        x: root.borderWidth
        y: root.contentBottom - height - root.borderWidth
        width: root.windowWidth - 2 * root.borderWidth
        height: stripButtons.implicitHeight
        color: Kirigami.Theme.backgroundColor

        WindowButtons {
            id: stripButtons

            anchors.right: parent.right
            anchors.rightMargin: Kirigami.Units.smallSpacing
            controller: app
            keepOpen: root.settings.keepOpen
        }
    }

    // Dragging the grip moves the bottom edge of the window.
    Rectangle {
        y: root.contentBottom
        width: root.windowWidth
        height: root.gripHeight
        color: Kirigami.Theme.backgroundColor

        Rectangle {
            anchors.centerIn: parent
            width: Kirigami.Units.gridUnit * 2
            height: 1
            color: Kirigami.Theme.disabledTextColor
        }

        MouseArea {
            anchors.fill: parent
            enabled: !app.fullscreen
            cursorShape: Qt.SizeVerCursor
            onPressedChanged: root.gripDragged = pressed
            onPositionChanged: mouse => {
                const bottom = mapToGlobal(mouse.x, mouse.y).y;
                const percent = Math.round(100 * bottom / root.workHeight);
                app.setHeightPercent(Math.max(10, Math.min(100, percent)));
            }
        }
    }

    Repeater {
        model: root.shortcuts

        delegate: Item {
            id: entry

            required property var modelData

            Shortcut {
                sequences: entry.modelData.sequences
                context: Qt.WindowShortcut
                enabled: !entry.modelData.name.startsWith("edit_find_") || root.searchActive
                onActivated: app.trigger(entry.modelData.name)
            }
        }
    }

    Component.onCompleted: {
        app.start();
        reportGeometry();
    }
}
