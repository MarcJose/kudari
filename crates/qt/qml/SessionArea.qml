// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import io.github.marcjose.kudari.qt

// The terminals of the active session as the controller lays them out, the lines between
// them that change their sizes, and the flash that shows where the focus went.
Item {
    id: area

    required property var controller
    required property var layoutData
    required property var settings
    required property font baseFont
    property bool applicationFocused: false
    // Terminals learn their new size only when this ends; every resize makes shells redraw.
    property bool resizePaused: false
    readonly property bool dragging: handleDrag.active
    property PaneTheme paneTheme: PaneTheme {}
    property color splitterColor: Qt.rgba(0.5, 0.5, 0.5, 1)
    property var panes: ({})

    signal searchOpenChanged(bool open)

    function update() {
        const ids = JSON.parse(controller.terminalIdsJson);
        for (const id of Object.keys(panes)) {
            if (!ids.includes(Number(id))) {
                panes[id].destroy();
                delete panes[id];
            }
        }
        for (const id of ids) {
            if (!panes[id]) {
                panes[id] = paneComponent.createObject(area, {terminalId: id, visible: false});
            }
        }
        arrange();
    }

    function arrange() {
        const shown = {};
        for (const entry of layoutData.terminals) {
            shown[entry.id] = entry;
        }
        for (const id of Object.keys(panes)) {
            const entry = shown[id];
            const pane = panes[id];
            pane.visible = entry !== undefined;
            if (entry) {
                pane.x = entry.x;
                pane.y = entry.y;
                pane.width = entry.width;
                pane.height = entry.height;
            }
        }
    }

    function highlight(terminalId, persistent, color, opacity, duration) {
        const entry = layoutData.terminals.find(terminal => terminal.id === terminalId);
        if (!entry || (!persistent && duration === 0)) {
            return;
        }
        flash.x = entry.x;
        flash.y = entry.y;
        flash.width = entry.width;
        flash.height = entry.height;
        flash.color = color;
        flash.opacity = opacity;
        flash.visible = true;
        flashTimer.stop();
        if (!persistent) {
            flashTimer.interval = duration;
            flashTimer.start();
        }
    }

    function clearHighlight() {
        flashTimer.stop();
        flash.visible = false;
    }

    onLayoutDataChanged: arrange()

    Component {
        id: paneComponent

        TerminalPane {
            theme: area.paneTheme
            isActive: terminalId === area.layoutData.active && area.applicationFocused
            keyboardEnabled: !(area.layoutData.keyboardDisabled ?? []).includes(terminalId)
            backgroundColor: area.settings.terminalBackground
            foregroundColor: area.settings.terminalForeground
            backgroundOpacity: area.settings.config.Opacity / 100
            controller: area.controller
            baseFont: area.baseFont
            fontZoom: area.layoutData.fontZoom?.[terminalId] ?? 0
            resizePaused: area.resizePaused || area.dragging
            config: area.settings.config
            onSearchOpenChanged: area.searchOpenChanged(searchOpen)
            focusFollowsMouse: area.settings.focusFollowsMouse
        }
    }

    QtObject {
        id: handleDrag

        property bool active: false
    }

    // The count is the model: an array model would recreate the delegates on every layout
    // change, ending a drag after one step.
    Repeater {
        model: area.layoutData.handles.length

        delegate: Rectangle {
            required property int index
            readonly property var modelData: area.layoutData.handles[index] ?? {x: 0, y: 0, width: 0, height: 0, horizontal: true, index: 0}

            x: modelData.x
            y: modelData.y
            z: 1
            width: modelData.width
            height: modelData.height
            color: area.splitterColor

            MouseArea {
                property point start

                anchors.fill: parent
                anchors.margins: -3
                cursorShape: parent.modelData.horizontal ? Qt.SplitHCursor : Qt.SplitVCursor
                onPressed: mouse => {
                    start = mapToItem(area, mouse.x, mouse.y);
                    handleDrag.active = true;
                }
                onReleased: handleDrag.active = false
                onCanceled: handleDrag.active = false
                onPositionChanged: mouse => {
                    const point = mapToItem(area, mouse.x, mouse.y);
                    const delta = parent.modelData.horizontal ? point.x - start.x : point.y - start.y;
                    if (delta !== 0) {
                        start = point;
                        area.controller.dragHandle(parent.modelData.index, delta);
                    }
                }
            }
        }
    }

    Rectangle {
        id: flash

        z: 2
        visible: false
    }

    Timer {
        id: flashTimer

        onTriggered: flash.visible = false
    }
}
