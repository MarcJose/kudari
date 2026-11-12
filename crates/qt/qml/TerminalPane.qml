// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import io.github.marcjose.kudari.qt

// One terminal with its scrollbar, search bar and overlays. The front end passes its colors
// and spacing in `theme`.
Item {
    id: pane

    // A button of the search bar: an icon of the theme, or a glyph of the icon font the
    // front end names, where there is no icon theme.
    component SearchButton: QQC2.ToolButton {
        required property string iconName
        required property string glyph
        property string label

        readonly property bool glyphs: pane.theme.iconFont.length > 0

        icon.name: glyphs ? "" : iconName
        text: glyphs ? glyph : label
        display: glyphs ? QQC2.AbstractButton.TextOnly : QQC2.AbstractButton.IconOnly
        QQC2.ToolTip.text: label
        QQC2.ToolTip.visible: hovered

        Binding on font.family {
            when: pane.theme.iconFont.length > 0
            value: pane.theme.iconFont
        }
    }

    required property int terminalId
    required property var controller
    required property font baseFont
    property int fontZoom: 0
    property bool resizePaused: false
    property bool focusFollowsMouse: false
    property bool isActive: false
    property bool keyboardEnabled: true
    property var config: ({})
    readonly property bool searchOpen: searchBar.visible
    property color backgroundColor: "#232627"
    property color foregroundColor: "#fcfcfc"
    property real backgroundOpacity: 1
    property PaneTheme theme: PaneTheme {}
    readonly property alias item: terminal

    onResizePausedChanged: {
        if (!resizePaused) {
            terminal.applySize();
        }
    }

    function focusTerminal() {
        terminal.forceActiveFocus();
    }

    function openSearch() {
        searchBar.visible = true;
        searchField.selectAll();
        searchField.forceActiveFocus();
    }

    function closeSearch() {
        searchBar.visible = false;
        terminal.endSearch();
        focusTerminal();
    }

    function search(backwards) {
        searchBar.failed = searchField.text.length > 0
            && !terminal.find(searchField.text, backwards, caseButton.checked, regexButton.checked, highlightButton.checked);
    }

    // F3 and Shift+F3 work with the bar closed, too.
    function findNext(backwards) {
        if (searchField.text.length === 0) {
            openSearch();
        } else {
            search(backwards);
        }
    }

    readonly property string layoutOptions: `${config.Margin}/${config.CenterContent}`

    onLayoutOptionsChanged: Qt.callLater(terminal.applySize)

    function colorUnderMouse(x, y) {
        const color = (config.ColorFilter ?? true) ? terminal.colorAt(x, y) : "";
        colorSwatch.visible = color.length > 0;
        if (colorSwatch.visible) {
            colorSwatch.color = color;
            colorSwatch.x = Math.min(x + pane.theme.gridUnit, terminal.width - colorSwatch.width);
            colorSwatch.y = Math.min(y + pane.theme.gridUnit, terminal.height - colorSwatch.height);
        }
    }

    function linkUnderMouse(x, y, modifiers) {
        const control = modifiers & Qt.ControlModifier;
        const wanted = control || (pane.config.UnderlineLinks ?? true);
        const geometry = wanted ? terminal.linkGeometry(x, y, control || (pane.config.UnderlineFiles ?? false)) : "";
        linkLine.visible = geometry.length > 0;
        if (linkLine.visible) {
            const parts = geometry.split(",").map(Number);
            linkLine.x = parts[0];
            linkLine.y = parts[1];
            linkLine.width = parts[2];
        }
    }

    FontMetrics {
        id: metrics

        font: Qt.font({
            family: pane.baseFont.family,
            pointSize: Math.max(4, pane.baseFont.pointSize + pane.fontZoom)
        })
    }

    // Unlike FontMetrics.advanceWidth(), this property updates with the font.
    TextMetrics {
        id: cellMetrics

        font: metrics.font
        text: "M"
    }

    // The scrollbar sits on the terminal background.
    Rectangle {
        anchors.fill: parent
        color: pane.backgroundColor
        opacity: pane.backgroundOpacity
    }

    TerminalItem {
        id: terminal

        anchors.fill: parent
        anchors.leftMargin: scrollBar.visible && pane.config.ScrollBarPosition === "left" ? scrollBar.width : 0
        anchors.rightMargin: scrollBar.visible && pane.config.ScrollBarPosition !== "left" ? scrollBar.width : 0
        terminalId: pane.terminalId
        termFont: metrics.font
        cellWidth: Math.round(cellMetrics.advanceWidth)
        cellHeight: Math.round(metrics.height) + (pane.config.LineSpacing ?? 0)
        boldIntense: pane.config.BoldIntense ?? true
        autoCopy: pane.config.AutoCopySelection ?? true
        alternateScrolling: pane.config.AlternateScrolling ?? true
        middleClickPaste: pane.config.MiddleClickPaste ?? "selection"
        ascent: Math.round(metrics.ascent)
        focused: pane.isActive
        focus: true
        keyboardEnabled: pane.keyboardEnabled
        opaque: pane.backgroundOpacity >= 1

        // Width and height change one after the other. A shell that saw the value in
        // between would redraw its prompt, so only the final size is reported.
        function applySize() {
            if (!pane.resizePaused && width > cellWidth && height > cellHeight) {
                resizeTo(width, height);
            }
        }

        onWidthChanged: Qt.callLater(applySize)
        onHeightChanged: Qt.callLater(applySize)
        onCellWidthChanged: Qt.callLater(applySize)
        onCellHeightChanged: Qt.callLater(applySize)
        onColumnsChanged: if (pane.visible && pane.width > 0) sizeHint.show()
        onLinesChanged: if (pane.visible && pane.width > 0) sizeHint.show()
        onCursorXChanged: Qt.inputMethod.update(Qt.ImCursorRectangle)
        onCursorYChanged: Qt.inputMethod.update(Qt.ImCursorRectangle)
        onActiveFocusChanged: {
            if (activeFocus) {
                pane.controller.activateTerminal(pane.terminalId);
            }
        }

        Keys.onReleased: event => {
            terminal.urlHints = false;
            if (event.key === Qt.Key_Control && mouse.containsMouse) {
                pane.linkUnderMouse(mouse.mouseX, mouse.mouseY, 0);
            }
        }
        Keys.onPressed: event => {
            cursorHidden = false;
            if (event.key === Qt.Key_Control && mouse.containsMouse) {
                pane.linkUnderMouse(mouse.mouseX, mouse.mouseY, Qt.ControlModifier);
            }
            if (!keyPress(event.key, event.modifiers, event.text)) {
                pane.controller.inputBlocked(pane.terminalId);
            }
            event.accepted = true;
        }
        Timer {
            interval: 500
            repeat: true
            running: terminal.cursorBlinks && terminal.focused && !(pane.config.AnimatedCursor ?? false)
            onRunningChanged: terminal.cursorHidden = false
            onTriggered: terminal.cursorHidden = !terminal.cursorHidden
        }

        // The fading variant of the blinking cursor.
        SequentialAnimation on cursorFade {
            running: terminal.cursorBlinks && terminal.focused && (pane.config.AnimatedCursor ?? false)
            loops: Animation.Infinite
            onRunningChanged: terminal.cursorFade = 1

            NumberAnimation { from: 1; to: 0; duration: 500; easing.type: Easing.InOutSine }
            NumberAnimation { from: 0; to: 1; duration: 500; easing.type: Easing.InOutSine }
        }

        // Carries selected text to other programs and terminals.
        Item {
            id: selectionDrag

            Drag.dragType: Drag.Automatic
            Drag.supportedActions: Qt.CopyAction
        }

        // Underlines a link while Ctrl is held over it.
        Rectangle {
            id: linkLine

            visible: false
            height: 1
            color: pane.foregroundColor
        }

        // Text an input method is still composing, shown at the terminal cursor.
        Rectangle {
            visible: terminal.preedit.length > 0
            x: preeditText.x
            y: preeditText.y
            width: preeditText.implicitWidth
            height: terminal.cellHeight
            color: pane.backgroundColor
        }

        Text {
            id: preeditText

            visible: terminal.preedit.length > 0
            x: terminal.cursorX
            y: terminal.cursorY
            text: terminal.preedit
            font.family: metrics.font.family
            font.pointSize: metrics.font.pointSize
            font.underline: true
            color: pane.foregroundColor
        }

        // Keeps a selection growing while the pointer is above or below the terminal.
        Timer {
            interval: 60
            repeat: true
            running: mouse.pressed && (mouse.mouseY < 0 || mouse.mouseY > terminal.height)
            onTriggered: {
                terminal.scrollBy(mouse.mouseY < 0 ? 1 : -1);
                terminal.mouseMove(mouse.mouseX, mouse.mouseY, 0);
            }
        }

        // Files dropped on the terminal arrive as quoted paths, ready for the shell.
        DropArea {
            anchors.fill: parent
            onDropped: drop => {
                if (drop.hasUrls) {
                    const paths = drop.urls.map(url => decodeURIComponent(url.toString().replace(/^file:\/\//, "")));
                    if (pane.config.DropUrlsAsText ?? true) {
                        terminal.dropText(paths.join("\n"), true);
                    } else {
                        const point = mapToItem(null, drop.x, drop.y);
                        pane.controller.popupDropMenu(point.x, point.y, pane.terminalId, paths.join("\n"));
                    }
                } else if (drop.hasText) {
                    terminal.dropText(drop.text, false);
                }
                drop.accept();
                terminal.forceActiveFocus();
            }
        }

        MouseArea {
            id: mouse

            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
            hoverEnabled: true
            cursorShape: linkLine.visible ? Qt.PointingHandCursor : Qt.IBeamCursor

            property int clicks: 1
            property real lastPress: 0
            // Where a press on the selection started; moving away from it drags the text.
            property var dragStart: null

            onExited: {
                linkLine.visible = false;
                colorSwatch.visible = false;
            }
            onEntered: {
                if (pane.focusFollowsMouse) {
                    terminal.forceActiveFocus();
                }
            }
            onPressed: event => {
                const needsControl = pane.config.DragNeedsCtrl ?? true;
                dragStart = event.button === Qt.LeftButton && (!needsControl || (event.modifiers & Qt.ControlModifier))
                    && !(event.modifiers & Qt.AltModifier) && terminal.hasSelection() && terminal.selectionContains(event.x, event.y)
                    ? Qt.point(event.x, event.y) : null;
                if (dragStart) {
                    return;
                }
                if (!terminal.activeFocus) {
                    pane.controller.terminalClicked(pane.terminalId);
                }
                terminal.forceActiveFocus();
                if (event.button === Qt.RightButton && !(event.modifiers & Qt.ShiftModifier)) {
                    return;
                }
                const now = Date.now();
                clicks = now - lastPress < Qt.styleHints.mouseDoubleClickInterval ? clicks % 3 + 1 : 1;
                lastPress = now;
                terminal.mousePress(event.x, event.y, event.button, clicks, event.modifiers);
            }
            onPositionChanged: event => {
                if (dragStart) {
                    if (Math.abs(event.x - dragStart.x) + Math.abs(event.y - dragStart.y) > Qt.styleHints.startDragDistance) {
                        selectionDrag.Drag.mimeData = {"text/plain": terminal.selectionText()};
                        selectionDrag.Drag.active = true;
                        dragStart = null;
                    }
                    return;
                }
                terminal.mouseMove(event.x, event.y, event.modifiers);
                pane.linkUnderMouse(event.x, event.y, event.modifiers);
                pane.colorUnderMouse(event.x, event.y);
            }
            onReleased: event => {
                selectionDrag.Drag.active = false;
                if (dragStart) {
                    dragStart = null;
                    return;
                }
                terminal.mouseRelease(event.x, event.y, event.button, event.modifiers);
            }
            onClicked: event => {
                const pasteMode = pane.config.RightClickPaste ?? "none";
                if (event.button === Qt.RightButton && pasteMode !== "none" && !(event.modifiers & Qt.ShiftModifier)) {
                    terminal.paste(pasteMode === "selection");
                } else if (event.button === Qt.RightButton) {
                    const point = mapToItem(null, event.x, event.y);
                    pane.controller.popupTerminalMenu(point.x, point.y, terminal.hasSelection(), terminal.linkAt(event.x, event.y));
                }
            }
            onWheel: event => {
                if ((event.modifiers & Qt.ControlModifier) && (pane.config.WheelZoom ?? true)) {
                    pane.controller.zoomTerminal(pane.terminalId, event.angleDelta.y > 0 ? 1 : -1);
                } else {
                    terminal.wheel(event.angleDelta.y, event.x, event.y);
                }
            }
        }
    }

    // Dims terminals without focus, if enabled.
    Rectangle {
        anchors.fill: parent
        visible: (pane.config.DimInactive ?? false) && !pane.isActive
        color: "black"
        opacity: (pane.config.DimValue ?? 77) / 255
    }

    // Previews the color named by the word under the mouse.
    Rectangle {
        id: colorSwatch

        visible: false
        width: pane.theme.gridUnit * 3
        height: pane.theme.gridUnit
        radius: 3
        border.color: pane.foregroundColor
    }

    // Marks the lines that came in while scrolling.
    Rectangle {
        id: scrollMark

        x: terminal.x
        y: terminal.scrollMarkY
        width: 3
        height: terminal.scrollMarkHeight
        color: pane.theme.highlightColor
        visible: scrollMarkTimer.running && (pane.config.HighlightScrolledLines ?? true)

        Timer {
            id: scrollMarkTimer

            interval: 250
        }

        Connections {
            target: terminal
            function onScrollMarkSerialChanged() {
                scrollMarkTimer.restart();
            }
        }
    }

    // Marks the terminal with the focus, if enabled.
    Rectangle {
        anchors.fill: parent
        visible: (pane.config.BorderWhenActive ?? false) && pane.isActive
        color: "transparent"
        border.width: 1
        border.color: pane.config.FocusBorderColor ?? "#3daee9"
    }

    // Shows the new size in columns and lines for a moment.
    Rectangle {
        id: sizeHint

        function show() {
            if (pane.config.ShowSizeHint ?? true) {
                visible = true;
                sizeHintTimer.restart();
            }
        }

        visible: false
        anchors.centerIn: parent
        width: sizeHintText.implicitWidth + pane.theme.largeSpacing * 2
        height: sizeHintText.implicitHeight + pane.theme.largeSpacing
        radius: pane.theme.cornerRadius
        color: pane.theme.backgroundColor

        QQC2.Label {
            id: sizeHintText

            anchors.centerIn: parent
            text: terminal.columns + " × " + terminal.lines
        }

        Timer {
            id: sizeHintTimer

            interval: 1000
            onTriggered: sizeHint.visible = false
        }
    }

    // Search bar at the bottom of the terminal.
    Rectangle {
        id: searchBar

        property bool failed: false

        visible: false
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: scrollBar.width
        height: searchRow.implicitHeight + pane.theme.smallSpacing * 2
        color: pane.theme.backgroundColor

        RowLayout {
            id: searchRow

            anchors.fill: parent
            anchors.margins: pane.theme.smallSpacing

            QQC2.TextField {
                id: searchField

                Layout.fillWidth: true
                placeholderText: pane.controller.translate("@info:placeholder", "Find…")
                color: searchBar.failed ? pane.theme.negativeTextColor : pane.theme.textColor
                onTextChanged: {
                    terminal.clearSelection();
                    pane.search(false);
                }
                Keys.onReturnPressed: event => pane.search(event.modifiers & Qt.ShiftModifier)
                Keys.onEnterPressed: event => pane.search(event.modifiers & Qt.ShiftModifier)
                Keys.onEscapePressed: pane.closeSearch()
            }

            SearchButton {
                id: caseButton

                checkable: true
                iconName: "format-text-superscript"
                glyph: "\uE8D2"
                label: pane.controller.translate("@option:check", "Match Case")
                onToggled: pane.search(false)
            }
            SearchButton {
                id: regexButton

                checkable: true
                iconName: "code-context"
                glyph: "\uE943"
                label: pane.controller.translate("@option:check", "Regular Expression")
                onToggled: pane.search(false)
            }
            SearchButton {
                id: highlightButton

                checkable: true
                checked: true
                iconName: "format-fill-color"
                glyph: "\uE7E6"
                label: pane.controller.translate("@option:check", "Highlight All Matches")
                onToggled: pane.search(false)
            }
            SearchButton {
                iconName: "go-up"
                glyph: "\uE70E"
                label: pane.controller.translate("@info:tooltip", "Find Previous")
                onClicked: pane.search(true)
            }
            SearchButton {
                iconName: "go-down"
                glyph: "\uE70D"
                label: pane.controller.translate("@info:tooltip", "Find Next")
                onClicked: pane.search(false)
            }
            SearchButton {
                iconName: "dialog-close"
                glyph: "\uE711"
                label: pane.controller.translate("@info:tooltip", "Close Search")
                onClicked: pane.closeSearch()
            }
        }
    }

    // A scrollbar in the style of Breeze. Without history the handle fills the rail.
    Item {
        id: scrollBar

        readonly property real railHeight: height - 6
        readonly property real markerHeight: Math.max(2, height * (pane.config.MarkerSize ?? 2) / 100)
        property var markers: []

        visible: pane.config.ScrollBarPosition !== "hidden"
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        x: pane.config.ScrollBarPosition === "left" ? 0 : pane.width - width
        width: 21

        Rectangle {
            x: 7
            y: 3
            width: 8
            height: scrollBar.railHeight
            radius: 4
            color: "#494b4c"
            border.color: "#5d6060"
            opacity: terminal.scrollSize < 1 ? 0.4 : 1
        }

        Rectangle {
            visible: terminal.scrollSize < 1
            x: 7
            y: 3 + terminal.scrollPosition * scrollBar.railHeight
            width: 8
            height: Math.max(20, terminal.scrollSize * scrollBar.railHeight)
            radius: 4
            color: "#494b4c"
            border.color: "#5d6060"
        }

        // Lines with search matches.
        Repeater {
            model: terminal.searchLines.length > 0 ? JSON.parse(terminal.searchLines) : []

            Rectangle {
                required property real modelData

                x: 2
                y: 3 + modelData * scrollBar.railHeight
                width: scrollBar.width - 4
                height: 1
                color: pane.config.SearchLineColor ?? "#ffff00"
                opacity: (pane.config.SearchLineOpacity ?? 100) / 100
            }
        }

        // Positions the user marked with a double click.
        Repeater {
            model: scrollBar.markers

            Rectangle {
                required property real modelData

                y: modelData
                width: scrollBar.width
                height: scrollBar.markerHeight
                color: pane.config.MarkerColor ?? "#ff0000"
                opacity: 0.75
            }
        }

        MouseArea {
            anchors.fill: parent
            onDoubleClicked: mouse => {
                const others = scrollBar.markers.filter(y => mouse.y < y || mouse.y >= y + scrollBar.markerHeight);
                scrollBar.markers = others.length < scrollBar.markers.length ? others : others.concat(mouse.y - scrollBar.markerHeight / 2);
            }
            function jump(y) {
                terminal.scrollToPosition(Math.max(0, Math.min(1, (y - 3) / scrollBar.railHeight - terminal.scrollSize / 2)));
            }
            onPressed: mouse => jump(mouse.y)
            onPositionChanged: mouse => jump(mouse.y)
            onWheel: wheel => terminal.wheel(wheel.angleDelta.y, 0, 0)
        }
    }
}
