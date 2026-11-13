// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import io.github.marcjose.kudari.qt
import io.github.marcjose.kudari.windows

// A borderless window at the top of the screen under the pointer, which slides in and out.
ApplicationWindow {
    id: root

    readonly property var settings: JSON.parse(app.settingsJson)
    readonly property var tabs: app.tabsJson.length > 0 ? JSON.parse(app.tabsJson) : []
    readonly property var layoutData: app.layoutJson.length > 0 ? JSON.parse(app.layoutJson) : ({terminals: [], handles: [], active: -1})
    readonly property var shortcuts: JSON.parse(app.shortcutsJson)

    readonly property var targetScreen: Qt.application.screens.find(candidate => candidate.name === app.outputName) ?? Qt.application.screens[0]
    readonly property int workWidth: !app.fullscreen && app.workWidth > 0 ? app.workWidth : targetScreen.width
    readonly property int workHeight: !app.fullscreen && app.workHeight > 0 ? app.workHeight : targetScreen.height
    readonly property int windowWidth: app.fullscreen ? workWidth : Math.floor(workWidth * settings.width / 100)
    readonly property int windowHeight: app.fullscreen ? workHeight : Math.floor(workHeight * settings.height / 100)
    readonly property int windowX: app.fullscreen ? 0 : Math.floor(workWidth * settings.position * (100 - settings.width) / 10000)
    // How far the window has slid in, from 0 to 1.
    property real shown: 0
    property bool searchActive: false
    // True while the grip is being dragged. Terminals then only learn their new size on
    // release; every resize makes shells redraw their prompt.
    property bool gripDragged: false
    // The icons of Windows as a font, or those of its older one; elsewhere the buttons show text.
    readonly property string iconFont: {
        const families = Qt.fontFamilies();
        return ["Segoe Fluent Icons", "Segoe MDL2 Assets"].find(family => families.includes(family)) ?? "";
    }
    // A terminal that shows through needs a window that does, too.
    readonly property bool translucent: settings.config.Opacity < 100
    property bool applicationFocused: false

    x: targetScreen.virtualX + windowX
    y: targetScreen.virtualY - Math.round((1 - shown) * windowHeight)
    width: windowWidth
    height: windowHeight
    visible: false
    flags: Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool
    color: translucent ? "transparent" : palette.window
    background: Rectangle {
        color: root.translucent ? "transparent" : root.palette.window
    }
    title: app.title.length > 0 ? app.title : "Kudari"

    onVisibleChanged: {
        if (visible) {
            app.afterShow();
        }
    }

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

    function reportGeometry() {
        if (sessionArea.width > 0 && sessionArea.height > 0 && terminalMetrics.height > 0) {
            app.setGeometry(sessionArea.width, sessionArea.height, Math.round(cellMetrics.advanceWidth), Math.round(terminalMetrics.height) + settings.config.LineSpacing);
        }
    }

    NumberAnimation {
        id: slide

        target: root
        property: "shown"
        duration: 150
        easing.type: Easing.OutCubic
        onFinished: {
            if (root.shown === 0) {
                root.visible = false;
            }
        }
    }

    function showOrHide(visible) {
        slide.stop();
        if (visible) {
            root.visible = true;
            root.requestActivate();
        }
        slide.to = visible ? 1 : 0;
        slide.start();
    }

    Controller {
        id: app

        onWindowVisibleChanged: root.showOrHide(windowVisible)
        onTerminalDirty: terminalId => {
            const pane = sessionArea.panes[terminalId];
            if (pane) {
                pane.item.refresh();
            }
        }
        onFocusTerminal: terminalId => focusLater.request(terminalId)
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
        onQuestionAsked: (id, title, text, confirm) => question.ask(id, title, text, confirm)
        onAgentAsked: (agent, title, text, allow, deny) => agentQuestion.ask(agent, title, text, allow, deny)
        onAgentQuestionClosed: agentQuestion.withdraw()
        onMenuRequested: (x, y, menu) => root.showMenu(x, y, JSON.parse(menu))
        onSettingsRequested: module => settingsWindow.show(module)
        onFirstRunRequested: welcome.open()
        onRenameRequested: session => tabBar.startRename(session)
        onQuitRequested: Qt.quit()
    }

    // The menus of the core, as data: see `kudari_core::menus`.
    Component {
        id: menuComponent

        // A window of its own, so that the menus of the bar reach below the window.
        Menu {
            popupType: Popup.Window
            // The style lets a menu grow to its height as it opens. In a window of its own,
            // that resizes the window with every frame, and the menu judders; so it opens
            // at its full height at once.
            enter: null
        }
    }

    Component {
        id: itemComponent

        // With its shortcut on the right, dimmed, as the menus of Windows show it.
        MenuItem {
            id: menuItem

            property string shortcut: ""

            rightPadding: 12 + (shortcut.length > 0 ? shortcutLabel.implicitWidth + 24 : 0)

            Label {
                id: shortcutLabel

                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                visible: menuItem.shortcut.length > 0
                text: menuItem.shortcut
                font: menuItem.font
                color: menuItem.palette.text
                opacity: menuItem.enabled ? 0.6 : 0.4
            }
        }
    }

    Component {
        id: separatorComponent

        MenuSeparator {}
    }

    function buildMenu(entries, parent, title) {
        const menu = menuComponent.createObject(parent, {title: title ?? ""});
        for (const entry of entries) {
            if (entry.type === "separator") {
                menu.addItem(separatorComponent.createObject(null));
            } else if (entry.type === "menu") {
                menu.addMenu(buildMenu(entry.entries, menu, entry.title));
            } else {
                const item = itemComponent.createObject(null, {
                    text: entry.text,
                    shortcut: root.shortcutText(entry),
                    checkable: entry.checkable === true || (entry.type === "value" && entry.kind !== "signal"),
                    checked: entry.checked === true,
                    enabled: entry.enabled !== false
                });
                item.triggered.connect(() => root.activate(entry, item.checked));
                menu.addItem(item);
            }
        }
        // The items arrive after the menu was made; it measures its height only once.
        menu.contentItem.forceLayout();
        // As wide as its widest entry: the style would cut long texts off. The width of an
        // entry leaves out the room that every entry keeps for a check mark and an arrow.
        let widest = 0;
        for (let index = 0; index < menu.count; ++index) {
            const item = menu.itemAt(index);
            const marks = (item.indicator?.width ?? 0) + (item.arrow?.width ?? 0) + 2 * (item.spacing ?? 0);
            widest = Math.max(widest, item.implicitWidth + marks);
        }
        menu.width = Math.min(widest + menu.leftPadding + menu.rightPadding, root.width);
        return menu;
    }

    // The first key of an action, as the user's language names it; none for other entries.
    function shortcutText(entry) {
        const shortcut = entry.type === "action" ? root.shortcuts.find(candidate => candidate.name === entry.name) : undefined;
        return shortcut && shortcut.sequences.length > 0 ? app.nativeKeySequence(shortcut.sequences[0]) : "";
    }

    function activate(entry, checked) {
        if (entry.name === "help_about_app") {
            about.open();
        } else if (entry.type === "action") {
            if (entry.checkable) {
                app.toggle(entry.name, checked);
            } else {
                app.trigger(entry.name);
            }
        } else if (entry.type === "value") {
            app.choose(entry.kind, entry.value);
        } else if (entry.type === "link") {
            app.useLink(entry.link, entry.copy);
        } else if (entry.type === "shell") {
            app.addSessionWithShell(entry.shell);
        } else if (entry.type === "tab-color") {
            if (entry.reset) {
                app.chooseTabColor(entry.session, "");
            } else {
                tabColor.session = entry.session;
                tabColor.selectedColor = entry.color.length > 0 ? entry.color : root.palette.accent;
                tabColor.open();
            }
        }
    }

    // Opens upwards where the screen ends below the point, as at the bar of the window, and
    // stays on the screen to the right.
    function showMenu(x, y, entries) {
        const menu = buildMenu(entries, root.contentItem);
        menu.closed.connect(() => menu.destroy());
        const below = targetScreen.virtualY + targetScreen.height - (root.y + y);
        const top = menu.implicitHeight > below ? y - menu.implicitHeight : y;
        const right = targetScreen.virtualX + targetScreen.width - root.x;
        menu.popup(Math.max(0, Math.min(x, right - menu.width)), top);
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
            // A dialog in the window keeps the keys.
            if (welcome.opened || question.opened || agentQuestion.opened || about.opened) {
                return;
            }
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
            return Qt.font({
                family: config.FontFamily.length > 0 ? config.FontFamily : (Qt.platform.os === "windows" ? "Consolas" : "monospace"),
                pointSize: config.FontSize > 0 ? config.FontSize : 10
            });
        }
        onFontChanged: root.reportGeometry()
    }

    TextMetrics {
        id: cellMetrics

        font: terminalMetrics.font
        text: "M"
        onAdvanceWidthChanged: root.reportGeometry()
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        spacing: 0

        SessionArea {
            id: sessionArea

            Layout.fillWidth: true
            Layout.fillHeight: true
            controller: app
            layoutData: root.layoutData
            settings: root.settings
            baseFont: terminalMetrics.font
            applicationFocused: root.applicationFocused
            resizePaused: root.gripDragged
            paneTheme.highlightColor: root.palette.highlight
            paneTheme.backgroundColor: root.palette.window
            paneTheme.textColor: root.palette.windowText
            paneTheme.iconFont: root.iconFont
            splitterColor: Qt.tint(root.palette.window, Qt.alpha(root.palette.windowText, 0.2))

            onWidthChanged: root.reportGeometry()
            onHeightChanged: root.reportGeometry()
            onSearchOpenChanged: open => root.searchActive = open
        }

        TabBar {
            id: tabBar

            Layout.fillWidth: true
            controller: app
            tabs: root.tabs
            iconFont: root.iconFont
            activityColor: root.settings.config.TabActivityColor
            keepOpen: root.settings.keepOpen
            showTabs: root.settings.showTabBar
        }

        // Dragging the grip moves the bottom edge of the window.
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 5
            color: root.palette.window

            Rectangle {
                anchors.centerIn: parent
                width: 32
                height: 1
                color: root.palette.placeholderText
            }

            MouseArea {
                anchors.fill: parent
                enabled: !app.fullscreen
                cursorShape: Qt.SizeVerCursor
                onPressedChanged: root.gripDragged = pressed
                onPositionChanged: mouse => {
                    const bottom = mapToGlobal(mouse.x, mouse.y).y - root.targetScreen.virtualY;
                    const percent = Math.round(100 * bottom / root.workHeight);
                    app.setHeightPercent(Math.max(10, Math.min(100, percent)));
                }
            }
        }
    }

    Dialog {
        id: question

        property int questionId: -1

        function ask(id, title, text, confirm) {
            questionId = id;
            question.title = title;
            body.text = text;
            open();
        }

        anchors.centerIn: parent
        modal: true
        focus: true
        standardButtons: Dialog.Yes | Dialog.Cancel
        onAccepted: app.answerQuestion(questionId, true)
        onRejected: app.answerQuestion(questionId, false)

        Label {
            id: body

            width: Math.min(implicitWidth, root.width * 0.6)
            wrapMode: Text.Wrap
            textFormat: Text.StyledText
        }
    }

    // Whether an AI agent may use Kudari; the window shows for it.
    Dialog {
        id: agentQuestion

        property string agent: ""

        function ask(agent, title, text, allow, deny) {
            agentQuestion.agent = agent;
            agentQuestion.title = title;
            agentBody.text = text;
            allowButton.text = allow;
            denyButton.text = deny;
            open();
            denyButton.forceActiveFocus();
        }

        // The agent went before the user answered.
        function withdraw() {
            agent = "";
            close();
        }

        anchors.centerIn: parent
        modal: true
        closePolicy: Popup.CloseOnEscape
        onRejected: {
            if (agent.length > 0) {
                app.answerAgent(agent, false);
            }
            agent = "";
        }
        onAccepted: {
            if (agent.length > 0) {
                app.answerAgent(agent, true);
            }
            agent = "";
        }

        footer: DialogButtonBox {
            Button {
                id: allowButton

                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
            }

            Button {
                id: denyButton

                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
            }
        }

        Label {
            id: agentBody

            width: Math.min(implicitWidth, root.width * 0.6)
            wrapMode: Text.Wrap
            textFormat: Text.StyledText
        }
    }

    // What Kudari is, who made it, and under which licenses.
    Dialog {
        id: about

        anchors.centerIn: parent
        modal: true
        focus: true
        title: "Kudari " + Qt.application.version
        standardButtons: Dialog.Close

        ColumnLayout {
            width: Math.min(520, root.width * 0.6)
            spacing: 12

            Label {
                Layout.fillWidth: true
                text: app.translate("@title", "A drop-down terminal for Windows and KDE Plasma")
                font.weight: Font.DemiBold
                wrapMode: Text.Wrap
            }

            Label {
                Layout.fillWidth: true
                text: "© 2026 Marc Jose · MIT\n" + app.translate("@info:credit", "Kudari started as a rewrite of Yakuake in Rust.")
                wrapMode: Text.Wrap
            }

            Label {
                Layout.fillWidth: true
                text: "Eike Hein and the Yakuake developers: " + app.translate("@info:credit", "Yakuake, the drop-down terminal Kudari grew out of")
                    + "\nThe Konsole developers: " + app.translate("@info:credit", "Ideas for many terminal options")
                wrapMode: Text.Wrap
                color: palette.placeholderText
            }

            Label {
                Layout.fillWidth: true
                text: app.translate("@info", "Kudari uses Qt under the LGPL 3. The licenses of Qt and of the other parts are in the folder licenses next to the program.")
                wrapMode: Text.Wrap
                color: palette.placeholderText
            }
        }
    }

    // The color of a tab, from its menu.
    ColorDialog {
        id: tabColor

        property int session: -1

        title: app.translate("@action", "Tab Color...").replace(/\s*(…|\.\.\.)$/, "")
        onAccepted: app.chooseTabColor(session, String(selectedColor))
    }

    // The first start offers another global shortcut: F12 alone belongs to the debugger on
    // Windows, so the default is Ctrl+F12.
    Dialog {
        id: welcome

        property string sequence

        anchors.centerIn: parent
        modal: true
        // The keys go to the dialog, not to the terminal behind it.
        focus: true
        closePolicy: Popup.NoAutoClose
        title: app.translate("@title", "Welcome to Kudari")
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAboutToShow: sequence = app.toggleShortcut()
        onAccepted: app.finishFirstRun(false, sequence)
        onRejected: app.finishFirstRun(false, "")

        ColumnLayout {
            width: Math.min(480, root.width * 0.6)
            spacing: 12

            Label {
                Layout.fillWidth: true
                text: app.translate("@info", "Kudari slides down and up again with a keyboard shortcut. You can pick another one now:")
                wrapMode: Text.Wrap
            }

            KeySequenceButton {
                id: welcomeKey

                Layout.alignment: Qt.AlignHCenter
                controller: app
                sequence: welcome.sequence
                onChosen: sequence => welcome.sequence = sequence
            }

            Shortcut {
                sequences: ["Return", "Enter"]
                enabled: welcome.opened && !welcomeKey.recording
                onActivated: welcome.accept()
            }

            Label {
                Layout.fillWidth: true
                text: app.translate("@info", "The menu lets you change it later as well.")
                wrapMode: Text.Wrap
            }
        }
    }

    SettingsWindow {
        id: settingsWindow

        controller: app
        iconFont: root.iconFont
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
