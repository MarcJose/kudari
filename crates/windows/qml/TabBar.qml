// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

// The sessions as tabs below the terminals, as in Yakuake and the KDE version, with buttons
// for a new session, the ways to open one, keeping the window open, and its menu. A double
// click renames a tab, its close button or a middle click closes it, a drag moves it, the
// wheel switches. Without `showTabs`, only the buttons remain.
Controls.Pane {
    id: bar

    // A button with a glyph of the icon font, or `fallback` without one.
    component IconButton: Controls.ToolButton {
        required property string glyph
        required property string fallback
        property string label

        text: bar.iconFont.length > 0 ? glyph : fallback
        Accessible.name: label
        Controls.ToolTip.text: label
        Controls.ToolTip.visible: hovered && label.length > 0

        Binding on font.family {
            when: bar.iconFont.length > 0
            value: bar.iconFont
        }
    }

    required property var controller
    required property var tabs
    required property string activityColor
    required property bool keepOpen
    property bool showTabs: true
    // A font of icons; without one, the buttons show text.
    property string iconFont: ""
    // Index of the tab being dragged, and the tab index it would land before (-1: at the end).
    property int dragIndex: -1
    property int dropBefore: -1

    // The tabs arrive as a new array on every change. Updating the entries in place keeps the
    // tab buttons, so that a drag survives an update.
    function syncTabs() {
        for (let index = 0; index < tabs.length; ++index) {
            const tab = tabs[index];
            const entry = {
                sessionId: tab.id,
                title: tab.title,
                selected: tab.selected,
                closable: tab.closable,
                activity: tab.activity,
                tabColor: tab.color ?? ""
            };
            if (index < tabModel.count) {
                tabModel.set(index, entry);
            } else {
                tabModel.append(entry);
            }
        }
        if (tabModel.count > tabs.length) {
            tabModel.remove(tabs.length, tabModel.count - tabs.length);
        }
    }

    function tabItem(index) {
        return index >= 0 && index < tabs.length ? repeater.itemAt(index) : null;
    }

    // The tab index a point in the bar falls before, -1 for the end, or -2 outside the tabs.
    function dropIndexAt(x, y) {
        if (y < 0 || y > height) {
            return -2;
        }
        for (let index = 0; index < tabs.length; ++index) {
            const item = tabItem(index);
            if (item && x < item.mapToItem(bar, item.width / 2, 0).x) {
                return index;
            }
        }
        return -1;
    }

    function finishDrag() {
        const source = dragIndex;
        const target = dropBefore;
        dragIndex = -1;
        dropBefore = -1;
        // Dropping a tab next to itself changes nothing.
        const unchanged = target === source || target === source + 1 || (target === -1 && source === tabs.length - 1);
        if (source >= 0 && target !== -2 && !unchanged) {
            controller.dropTab(tabs[source].id, target);
        }
    }

    function startRename(session) {
        const index = tabs.findIndex(tab => tab.id === session);
        const item = tabItem(index);
        if (!item) {
            return;
        }
        const place = item.mapToItem(renameField.parent, 0, 0);
        renameField.session = session;
        renameField.x = place.x;
        renameField.y = place.y;
        renameField.width = Math.max(item.width, 120);
        renameField.height = item.height;
        renameField.text = tabs[index].title;
        renameField.visible = true;
        renameField.selectAll();
        renameField.forceActiveFocus();
    }

    onTabsChanged: syncTabs()
    Component.onCompleted: syncTabs()
    padding: 2
    // The pane measures only a single child; this one has more.
    contentWidth: row.implicitWidth
    contentHeight: row.implicitHeight

    ListModel {
        id: tabModel
    }

    WheelHandler {
        enabled: bar.showTabs
        onWheel: event => {
            if (event.angleDelta.y < 0) {
                bar.controller.selectNextTab();
            } else if (event.angleDelta.y > 0) {
                bar.controller.selectPreviousTab();
            }
        }
    }

    RowLayout {
        id: row

        anchors.fill: parent
        spacing: 2

        // New sessions on the left of the tabs, as in the KDE version and in Yakuake.
        IconButton {
            glyph: "\uE710"
            fallback: "+"
            label: bar.controller.translate("@action", "New Session")
            onClicked: bar.controller.addSessionAction()
        }

        IconButton {
            glyph: "\uE70D"
            fallback: "⌄"
            label: bar.controller.translate("@title:menu", "New Session With")
            onClicked: {
                const point = mapToItem(null, 0, 0);
                bar.controller.popupSessionMenu(point.x, point.y);
            }
        }

        Controls.TabBar {
            id: tabStrip

            readonly property int selected: bar.tabs.findIndex(tab => tab.selected)

            Layout.fillWidth: true
            Layout.maximumWidth: contentWidth
            // As high as its tabs: the style gives the strip 48 pixels.
            implicitHeight: contentHeight
            topPadding: 0
            bottomPadding: 0
            visible: bar.showTabs
            currentIndex: selected
            // The wheel switches tabs instead of scrolling the row of them; the row follows.
            Component.onCompleted: contentItem.interactive = false
            onSelectedChanged: {
                if (selected >= 0) {
                    contentItem.positionViewAtIndex(selected, ListView.Contain);
                }
            }

            Repeater {
                id: repeater

                model: tabModel

                delegate: Controls.TabButton {
                    id: tab

                    required property int index
                    required property int sessionId
                    required property string title
                    required property bool selected
                    required property bool closable
                    required property bool activity
                    required property string tabColor

                    width: implicitWidth
                    implicitWidth: leftPadding + contentItem.implicitWidth + rightPadding
                    // As high as the tool buttons beside them, as in the tab rows of Windows:
                    // the style makes tabs 40 pixels high, which the bar of a terminal that
                    // is always on screen does not need.
                    implicitHeight: 32
                    leftPadding: 12
                    rightPadding: closable ? 4 : 12
                    topPadding: 4
                    bottomPadding: 4
                    Accessible.name: title
                    opacity: bar.dragIndex === index ? 0.5 : 1
                    onClicked: bar.controller.selectTab(sessionId)
                    onDoubleClicked: bar.startRename(sessionId)

                    contentItem: RowLayout {
                        spacing: 6

                        Controls.Label {
                            visible: !tab.closable
                            text: bar.iconFont.length > 0 ? "\uE72E" : "🔒"
                            font.family: bar.iconFont.length > 0 ? bar.iconFont : font.family
                            font.pixelSize: 12
                        }

                        Controls.Label {
                            // Long terminal titles are shortened so that more tabs fit.
                            Layout.maximumWidth: 220
                            text: tab.title
                            elide: Text.ElideMiddle
                            color: tab.activity && bar.activityColor.length > 0 ? bar.activityColor : tab.palette.buttonText
                            font.italic: tab.activity && bar.activityColor.length === 0
                        }

                        Controls.ToolButton {
                            Layout.preferredWidth: 20
                            Layout.preferredHeight: 20
                            visible: tab.closable
                            flat: true
                            // The style sets each side on its own, which padding does not undo;
                            // in a button this small, its 8 pixels would leave no room.
                            topPadding: 0
                            bottomPadding: 0
                            leftPadding: 0
                            rightPadding: 0
                            text: bar.iconFont.length > 0 ? "\uE711" : "✕"
                            font.family: bar.iconFont.length > 0 ? bar.iconFont : font.family
                            font.pixelSize: 10
                            Accessible.name: bar.controller.translate("@info:tooltip", "Close Session")
                            Controls.ToolTip.text: Accessible.name
                            Controls.ToolTip.visible: hovered
                            onClicked: bar.controller.closeTab(tab.sessionId)
                        }
                    }

                    TapHandler {
                        acceptedButtons: Qt.MiddleButton
                        onTapped: bar.controller.closeTab(tab.sessionId)
                    }

                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: eventPoint => {
                            const point = tab.mapToItem(null, eventPoint.position.x, eventPoint.position.y);
                            bar.controller.popupTabMenu(tab.sessionId, point.x, point.y);
                        }
                    }

                    DragHandler {
                        target: null
                        yAxis.enabled: false
                        onActiveChanged: {
                            if (active) {
                                bar.dragIndex = tab.index;
                            } else {
                                bar.finishDrag();
                            }
                        }
                        onCentroidChanged: {
                            if (active) {
                                const point = tab.mapToItem(bar, centroid.position.x, centroid.position.y);
                                bar.dropBefore = bar.dropIndexAt(point.x, point.y);
                            }
                        }
                    }

                    // The color the user gave this tab.
                    Rectangle {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: 4
                        height: 2
                        radius: 1
                        visible: tab.tabColor.length > 0
                        color: tab.tabColor
                    }
                }
            }
        }

        // The free space: a double click opens a session, a right click shows the session menu.
        MouseArea {
            Layout.fillWidth: true
            Layout.fillHeight: true
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onDoubleClicked: mouse => {
                if (mouse.button === Qt.LeftButton) {
                    bar.controller.addSessionAction();
                }
            }
            onClicked: mouse => {
                if (mouse.button === Qt.RightButton) {
                    const point = mapToItem(null, mouse.x, mouse.y);
                    bar.controller.popupSessionMenu(point.x, point.y);
                }
            }
        }

        // Shows while an AI agent may use Kudari; a click ends its access.
        IconButton {
            visible: bar.controller.agentAccess
            highlighted: true
            glyph: "\uE890"
            fallback: "◉"
            label: bar.controller.translate("@info:tooltip", "An AI agent has access to your terminals. Click to end its access.")
            onClicked: bar.controller.revokeAgentAccess()
        }

        IconButton {
            checkable: true
            checked: bar.keepOpen
            glyph: bar.keepOpen ? "\uE840" : "\uE718"
            fallback: "📌"
            label: bar.controller.translate("@info:tooltip", "Keep open when the focus moves elsewhere")
            onClicked: bar.controller.setKeepOpen(checked)
        }

        IconButton {
            glyph: "\uE700"
            fallback: "≡"
            label: bar.controller.translate("@info:tooltip", "Open Menu")
            onClicked: {
                const point = mapToItem(null, 0, 0);
                bar.controller.popupMainMenu(point.x, point.y);
            }
        }

        IconButton {
            glyph: "\uE8BB"
            fallback: "✕"
            label: bar.controller.translate("@info:tooltip Quits the application", "Quit")
            onClicked: bar.controller.trigger("file_quit")
        }
    }

    // Where a dragged tab would land.
    Rectangle {
        readonly property var before: bar.dropBefore >= 0 ? bar.tabItem(bar.dropBefore) : bar.tabItem(bar.tabs.length - 1)

        visible: bar.dragIndex >= 0 && bar.dropBefore !== -2 && before !== null
        width: 2
        height: bar.height
        x: before ? before.mapToItem(bar, bar.dropBefore >= 0 ? 0 : before.width, 0).x - width / 2 : 0
        y: -bar.topPadding
        color: bar.palette.accent
    }

    // Fluent draws text fields see-through; the tab beneath must not show.
    Rectangle {
        visible: renameField.visible
        x: renameField.x
        y: renameField.y
        width: renameField.width
        height: renameField.height
        radius: 4
        color: bar.palette.window
    }

    Controls.TextField {
        id: renameField

        property int session: -1

        visible: false
        horizontalAlignment: TextInput.AlignHCenter
        onAccepted: {
            visible = false;
            bar.controller.renameTab(session, text);
        }
        onActiveFocusChanged: {
            if (!activeFocus) {
                visible = false;
            }
        }
        Keys.onEscapePressed: {
            visible = false;
            bar.controller.selectTab(session);
        }
    }
}
