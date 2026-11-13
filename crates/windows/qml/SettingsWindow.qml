// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The settings, laid out like those of Windows 11: the pages on the left, one page on the
// right. The pages are those of kudari-qt, the same as on KDE.
ApplicationWindow {
    id: root

    required property var controller
    readonly property var modules: [
        {id: "window", page: "WindowSettingsPage", glyph: "\uE737", text: controller.translate("@title Preferences page name", "Window")},
        {id: "behavior", page: "BehaviorSettingsPage", glyph: "\uE713", text: controller.translate("@title Preferences page name", "Behavior")},
        {id: "appearance", page: "AppearanceSettingsPage", glyph: "\uE790", text: controller.translate("@title Preferences page name", "Appearance")},
        {id: "terminal", page: "TerminalSettingsPage", glyph: "\uE756", text: controller.translate("@title Preferences page name", "Terminal")},
        {id: "terminal-behavior", page: "TerminalBehaviorPage", glyph: "\uE962", text: controller.translate("@title Preferences page name", "Terminal Behavior")},
        {id: "security", page: "SecuritySettingsPage", glyph: "\uE72E", text: controller.translate("@title Preferences page name", "Security")},
        {id: "shortcuts", page: "ShortcutsPage", glyph: "\uE765", text: controller.translate("@title Preferences page name", "Shortcuts"), windows: true}
    ]
    property string iconFont
    property int current: 0

    // Opens on the page with the id `module`, or on the first, in the middle of the screen:
    // the window of the terminals, which Qt would center it on, may still be sliding in.
    function show(module) {
        current = Math.max(0, modules.findIndex(entry => entry.id === module));
        controller.dialogOpen = true;
        if (!visible) {
            x = screen.virtualX + Math.round((screen.width - width) / 2);
            y = screen.virtualY + Math.round((screen.height - height) / 2);
        }
        visible = true;
        raise();
        requestActivate();
        navigation.forceActiveFocus();
    }

    width: 960
    height: 680
    minimumWidth: 640
    minimumHeight: 400
    title: controller.translate("@title:menu", "Settings")
    onClosing: controller.dialogOpen = false

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ListView {
            id: navigation

            Layout.preferredWidth: 280
            Layout.fillHeight: true
            Layout.topMargin: 12
            model: root.modules
            currentIndex: root.current
            onCurrentIndexChanged: root.current = currentIndex
            focus: true
            keyNavigationEnabled: true
            spacing: 2

            delegate: ItemDelegate {
                id: entry

                required property var modelData
                required property int index

                x: 8
                width: ListView.view.width - 16
                // For screen readers and UI Automation; the content shows it with its icon.
                text: modelData.text
                highlighted: ListView.isCurrentItem
                onClicked: root.current = index

                contentItem: RowLayout {
                    spacing: 12

                    Label {
                        visible: root.iconFont.length > 0
                        text: entry.modelData.glyph
                        font.family: root.iconFont
                        font.pixelSize: 16
                    }

                    Label {
                        Layout.fillWidth: true
                        text: entry.modelData.text
                        elide: Text.ElideRight
                    }
                }
            }
        }

        Loader {
            id: page

            Layout.fillWidth: true
            Layout.fillHeight: true
        }
    }

    // Each page is built anew when it is chosen, so that it shows the settings as they are.
    // Those of kudari-qt are shared with KDE, the others belong to Windows.
    function load() {
        const module = modules[current];
        if (module.windows) {
            page.setSource("qrc:/qt/qml/io/github/marcjose/kudari/windows/qml/" + module.page + ".qml", {controller: controller, iconFont: iconFont});
        } else {
            page.setSource("qrc:/qt/qml/io/github/marcjose/kudari/qt/qml/" + module.page + ".qml", {controller: controller});
        }
    }

    onCurrentChanged: {
        if (visible) {
            load();
        }
    }
    onVisibleChanged: {
        if (visible) {
            load();
        } else {
            page.source = "";
        }
    }
}
