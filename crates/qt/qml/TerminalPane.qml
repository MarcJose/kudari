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
        // A screen reader would read the glyph otherwise.
        Accessible.name: label

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
    // Whether anyone can see the terminal: its tab is the one shown, and so is the window.
    readonly property bool seen: visible && Window.window !== null && Window.window.visible

    onResizePausedChanged: {
        if (!resizePaused) {
            terminal.applySize();
        }
    }

    // Ends the rest of the cursor and starts its blinking anew, shown: after a key, the cursor
    // stays for half a cycle instead of vanishing at once.
    function wakeCursor() {
        terminal.cursorResting = false;
        terminal.cursorHidden = false;
        if (cursorRest.running) {
            cursorRest.restart();
        }
        if (cursorBlink.running) {
            cursorBlink.restart();
        }
        if (cursorFading.running) {
            cursorFading.reset();
        }
    }

    // Takes in output: a terminal that nobody sees only notes it and catches up when it shows
    // again, instead of taking the lock and drawing a frame for every piece of output in a
    // tab in the background or a window that is hidden.
    function refreshWhenSeen() {
        if (seen) {
            terminal.stale = false;
            terminal.refresh();
        } else {
            terminal.stale = true;
        }
    }

    onSeenChanged: {
        if (seen && terminal.stale) {
            terminal.stale = false;
            // Once the terminal has its size again, which comes with the same change.
            Qt.callLater(terminal.refresh);
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
        if (searchField.text.length === 0) {
            // An empty field finds nothing, and the matches of the text before go.
            terminal.clearSearch();
            searchBar.failed = false;
            return;
        }
        searchBar.failed = !terminal.find(searchField.text, backwards, caseButton.checked, regexButton.checked, highlightButton.checked);
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

        // Whether the terminal got its first size: that one is no change to show.
        property bool sized: false
        // Whether output came while nobody saw the terminal; see refreshWhenSeen().
        property bool stale: false

        // The room of the scrollbar beside the terminal. From the setting, not from
        // scrollBar.visible: that turns false with a hidden tab, and the terminal would get more
        // columns there and wrap again on every switch of tabs.
        readonly property real leftGap: pane.config.ScrollBarPosition === "left" ? scrollBar.width : 0
        readonly property real rightGap: pane.config.ScrollBarPosition !== "left" && pane.config.ScrollBarPosition !== "hidden" ? scrollBar.width : 0

        objectName: "terminal"
        // A terminal in a tab in the background has no size: Qt then frees its image and its
        // texture, some MB each, and draws nothing. It keeps its columns and lines, since
        // applySize() leaves out any size below a cell, and catches up when it shows.
        x: leftGap
        width: pane.visible ? Math.max(0, parent.width - leftGap - rightGap) : 0
        height: pane.visible ? parent.height : 0
        terminalId: pane.terminalId
        termFont: metrics.font
        // Cells of whole pixels of the device, also at a scale such as 125 %: every cell then
        // starts on a pixel, and the lines of tables are as wide in each.
        pixelRatio: Screen.devicePixelRatio
        cellWidth: Math.max(1, Math.round(cellMetrics.advanceWidth * pixelRatio)) / pixelRatio
        cellHeight: Math.max(1, Math.round((Math.round(metrics.height) + (pane.config.LineSpacing ?? 0)) * pixelRatio)) / pixelRatio
        boldIntense: pane.config.BoldIntense ?? true
        autoCopy: pane.config.AutoCopySelection ?? true
        alternateScrolling: pane.config.AlternateScrolling ?? true
        middleClickPaste: pane.config.MiddleClickPaste ?? "selection"
        ascent: Math.round(metrics.ascent * pixelRatio) / pixelRatio
        focused: pane.isActive
        focus: true
        keyboardEnabled: pane.keyboardEnabled
        opaque: pane.backgroundOpacity >= 1

        // Width and height change one after the other. A shell that saw the value in
        // between would redraw its prompt, so only the final size is reported.
        // Below a cell, as in a tab in the background, the terminal keeps its size: the program
        // in it gets no change of size and wraps nothing.
        function applySize() {
            if (!pane.resizePaused && width > cellWidth && height > cellHeight) {
                // columns and lines change within `resizeTo`, still with sized false.
                resizeTo(width, height);
                sized = true;
            }
        }

        onWidthChanged: Qt.callLater(applySize)
        onHeightChanged: Qt.callLater(applySize)
        onCellWidthChanged: Qt.callLater(applySize)
        onCellHeightChanged: Qt.callLater(applySize)
        onColumnsChanged: if (sized && pane.visible && pane.width > 0) sizeHint.show()
        onLinesChanged: if (sized && pane.visible && pane.width > 0) sizeHint.show()
        // The cursor changes column and line one after the other, while the terminal is being
        // refreshed: the input method hears of it once, afterwards, with both.
        function reportCursor() {
            if (activeFocus) {
                Qt.inputMethod.update(Qt.ImCursorRectangle);
            }
        }

        // Output that moves the cursor also wakes it.
        onCursorXChanged: {
            Qt.callLater(reportCursor);
            pane.wakeCursor();
        }
        onCursorYChanged: {
            Qt.callLater(reportCursor);
            pane.wakeCursor();
        }
        // So does getting the focus back.
        onFocusedChanged: {
            if (focused) {
                pane.wakeCursor();
            }
        }
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
            pane.wakeCursor();
            if (event.key === Qt.Key_Control && mouse.containsMouse) {
                pane.linkUnderMouse(mouse.mouseX, mouse.mouseY, Qt.ControlModifier);
            }
            if (!keyPress(event.key, event.modifiers, event.text, event.nativeScanCode)) {
                pane.controller.inputBlocked(pane.terminalId);
            }
            event.accepted = true;
        }
        // Finds the places of the matches for the scrollbar again after output, at most four
        // times a second.
        Timer {
            interval: 250
            running: terminal.searchLinesStale
            onTriggered: terminal.updateSearchLines()
        }

        // The cursor rests, steady and shown, after a while without input or output, as in
        // GTK and kitty: a blinking cursor in a terminal nobody uses only costs power.
        property bool cursorResting: false
        // Whether the cursor blinks at all: as the program or the setting asks, unless the
        // system turned blinking off (a time of 0), which it does for those whom it disturbs.
        readonly property bool cursorBlinking: cursorBlinks && focused && !cursorResting && Qt.styleHints.cursorFlashTime > 0

        Timer {
            id: cursorRest

            interval: (pane.config.CursorBlinkTimeout ?? 15) * 1000
            running: terminal.cursorBlinks && terminal.focused && !terminal.cursorResting && interval > 0
            onTriggered: terminal.cursorResting = true
        }

        // Whether the cursor blinks starts to change while the terminal is being refreshed;
        // the cursor shows again after that. Half a cycle of the system shown, half hidden.
        Timer {
            id: cursorBlink

            function reset() {
                terminal.cursorHidden = false;
            }

            interval: Math.max(1, Qt.styleHints.cursorFlashTime / 2)
            repeat: true
            running: terminal.cursorBlinking && !(pane.config.AnimatedCursor ?? false)
            onRunningChanged: Qt.callLater(reset)
            onTriggered: terminal.cursorHidden = !terminal.cursorHidden
        }

        // The fading variant of the blinking cursor, in a cycle of the system. Its brightness
        // comes from the time that passed: an animation of Qt Quick moves with the frames of
        // the primary screen, too fast or in steps when Kudari is on a screen with another rate.
        Timer {
            id: cursorFading

            property real start: 0

            function reset() {
                start = Date.now();
                terminal.cursorFade = 1;
            }

            interval: 33
            repeat: true
            running: terminal.cursorBlinking && (pane.config.AnimatedCursor ?? false)
            onRunningChanged: Qt.callLater(reset)
            onTriggered: terminal.cursorFade = 0.5 + 0.5 * Math.cos(2 * Math.PI * (Date.now() - start) / Math.max(1, Qt.styleHints.cursorFlashTime))
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
                    // Qt knows the paths of the platform: C:\ on Windows, any name on Linux.
                    const paths = terminal.localPaths(drop.urls);
                    if (pane.config.DropUrlsAsText ?? true) {
                        terminal.dropText(paths, true);
                    } else {
                        const point = mapToItem(null, drop.x, drop.y);
                        pane.controller.popupDropMenu(point.x, point.y, pane.terminalId, paths);
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
            // Only a press near the last one, with the same button, counts on from it.
            property point lastPoint: Qt.point(0, 0)
            property int lastButton: Qt.NoButton
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
                const distance = Qt.styleHints.mouseDoubleClickDistance;
                const near = Math.abs(event.x - lastPoint.x) <= distance && Math.abs(event.y - lastPoint.y) <= distance;
                const again = near && event.button === lastButton && now - lastPress < Qt.styleHints.mouseDoubleClickInterval;
                clicks = again ? clicks % 3 + 1 : 1;
                lastPress = now;
                lastPoint = Qt.point(event.x, event.y);
                lastButton = event.button;
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
                    const steps = terminal.wheelZoom(event.angleDelta.y, event.pixelDelta.y);
                    if (steps !== 0)
                        pane.controller.zoomTerminal(pane.terminalId, steps);
                } else {
                    terminal.wheel(event.angleDelta.y, event.pixelDelta.y, event.x, event.y);
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

        objectName: "sizeHint"
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

        objectName: "searchBar"
        visible: false
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        // As wide as the terminal, beside the scrollbar wherever that is.
        anchors.leftMargin: terminal.leftGap
        anchors.rightMargin: terminal.rightGap
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
            onWheel: wheel => terminal.wheel(wheel.angleDelta.y, wheel.pixelDelta.y, 0, 0)
        }
    }
}
