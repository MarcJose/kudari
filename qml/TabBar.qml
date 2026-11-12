// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The bar at the bottom of the window: a button for a new session, one tab per session, and the
// buttons of the window. Tabs scroll when they do not fit. A double click renames a tab, its
// close button or a middle click closes it, a drag moves it, the wheel switches.
Item {
    id: bar

    required property var controller
    required property var fonts
    required property var tabs
    required property string activityColor
    required property bool keepOpen

    // Index of the tab being dragged, and the tab index it would land before (-1: at the end).
    property int dragIndex: -1
    property int dropBefore: -1

    implicitHeight: row.implicitHeight

    // The tabs arrive as a new array on every change. Updating the entries in place keeps the
    // tab buttons, so a drag survives an update and Breeze always has a tab to measure.
    function syncTabs() {
        for (let index = 0; index < tabs.length; ++index) {
            const tab = tabs[index];
            const entry = {
                sessionId: tab.id,
                title: tab.title,
                selected: tab.selected,
                closable: tab.closable,
                activity: tab.activity,
                tabColor: tab.color
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

    onTabsChanged: syncTabs()
    Component.onCompleted: syncTabs()

    ListModel {
        id: tabModel
    }

    function tabItem(index) {
        return index >= 0 && tabs.length > index && tabRow.item ? tabRow.item.tabAt(index) : null;
    }

    // The tab index a point in the bar falls before, -1 for the end, or -2 outside the tabs.
    function dropIndexAt(x, y) {
        if (y < 0 || y > height) {
            return -2;
        }
        for (let index = 0; index < tabs.length; ++index) {
            const item = tabItem(index);
            if (!item) {
                continue;
            }
            const center = item.mapToItem(bar, item.width / 2, 0).x;
            if (x < center) {
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
        const place = item.mapToItem(bar, 0, 0);
        renameField.session = session;
        renameField.x = place.x;
        renameField.y = place.y;
        renameField.width = Math.max(item.width, Kirigami.Units.gridUnit * 6);
        renameField.height = item.height;
        renameField.text = tabs[index].title;
        renameField.visible = true;
        renameField.selectAll();
        renameField.forceActiveFocus();
    }

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor
    }

    WheelHandler {
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
        spacing: 0

        // Press and hold for the menu of session layouts.
        QQC2.ToolButton {
            icon.name: "list-add"
            display: QQC2.AbstractButton.IconOnly
            text: bar.controller.translate("@info:tooltip", "New Session")
            QQC2.ToolTip.text: text
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
            onClicked: bar.controller.addSessionAction()
            onPressAndHold: {
                const point = mapToItem(null, 0, height);
                bar.controller.popupSessionMenu(point.x, point.y);
            }
        }

        // Breeze sizes the bar by its first tab, so it only exists while there is one.
        Loader {
            id: tabRow

            Layout.fillWidth: true
            Layout.maximumWidth: item ? item.implicitWidth : 0
            Layout.preferredHeight: item ? item.implicitHeight : 0
            active: tabModel.count > 0
            sourceComponent: tabsComponent
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

        WindowButtons {
            Layout.rightMargin: Kirigami.Units.smallSpacing
            controller: bar.controller
            keepOpen: bar.keepOpen
        }
    }

    Component {
        id: tabsComponent

        QQC2.TabBar {
            id: tabStrip

            readonly property int selected: bar.tabs.findIndex(tab => tab.selected)

            function tabAt(index) {
                return repeater.itemAt(index);
            }

            function scrollToSelected() {
                if (selected >= 0) {
                    contentItem.positionViewAtIndex(selected, ListView.Contain);
                }
            }

            onSelectedChanged: {
                setCurrentIndex(selected);
                scrollToSelected();
            }
            onWidthChanged: scrollToSelected()

            // A new tab learns its width only after it was selected.
            Connections {
                target: tabStrip.contentItem

                function onContentWidthChanged() {
                    Qt.callLater(tabStrip.scrollToSelected);
                }
            }
            // The wheel switches tabs instead of scrolling the row of them; the row follows.
            Component.onCompleted: contentItem.interactive = false

            Repeater {
                id: repeater

                model: tabModel

                delegate: QQC2.TabButton {
                    id: tab

                    required property int index
                    required property int sessionId
                    required property string title
                    required property bool selected
                    required property bool closable
                    required property bool activity
                    required property string tabColor

                    // Breeze draws the text of a tab centered; the tab draws its own content so
                    // that the title starts at the left and the close button has its place.
                    implicitWidth: leftPadding + contentItem.implicitWidth + rightPadding
                    width: implicitWidth
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: closable ? Kirigami.Units.smallSpacing : Kirigami.Units.largeSpacing
                    Accessible.name: title
                    opacity: bar.dragIndex === index ? 0.5 : 1
                    onClicked: bar.controller.selectTab(sessionId)
                    onDoubleClicked: bar.startRename(sessionId)
                    Component.onCompleted: if (selected) {
                        tabStrip.setCurrentIndex(index);
                    }

                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Icon {
                            Layout.preferredWidth: Kirigami.Units.iconSizes.small
                            Layout.preferredHeight: Kirigami.Units.iconSizes.small
                            visible: !tab.closable
                            source: "object-locked"
                        }

                        QQC2.Label {
                            // Long terminal titles are shortened so that more tabs fit.
                            Layout.maximumWidth: Kirigami.Units.gridUnit * 12
                            text: tab.title
                            elide: Text.ElideMiddle
                            font.family: bar.fonts.general.family
                            font.pointSize: bar.fonts.general.pointSize
                            color: tab.activity ? bar.activityColor : Kirigami.Theme.textColor
                        }

                        QQC2.ToolButton {
                            Layout.preferredWidth: Kirigami.Units.iconSizes.small + Kirigami.Units.smallSpacing
                            Layout.preferredHeight: Layout.preferredWidth
                            visible: tab.closable
                            flat: true
                            padding: 0
                            icon.name: "window-close"
                            icon.width: Kirigami.Units.iconSizes.small
                            icon.height: Kirigami.Units.iconSizes.small
                            display: QQC2.AbstractButton.IconOnly
                            text: bar.controller.translate("@info:tooltip", "Close Session")
                            QQC2.ToolTip.text: text
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
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
                        height: Math.round(Kirigami.Units.smallSpacing / 2)
                        visible: tab.tabColor.length > 0
                        color: tab.tabColor
                    }
                }
            }
        }
    }

    // Where a dragged tab would land.
    Rectangle {
        readonly property var before: bar.dropBefore >= 0 ? bar.tabItem(bar.dropBefore) : bar.tabItem(bar.tabs.length - 1)

        visible: bar.dragIndex >= 0 && bar.dropBefore !== -2 && before !== null
        width: Math.max(2, Math.round(Kirigami.Units.smallSpacing / 2))
        height: bar.height
        x: before ? before.mapToItem(bar, bar.dropBefore >= 0 ? 0 : before.width, 0).x - width / 2 : 0
        color: Kirigami.Theme.highlightColor
    }

    QQC2.TextField {
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
