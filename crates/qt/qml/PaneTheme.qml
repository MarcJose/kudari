// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick

// The colors and spacing a terminal pane takes from its front end, by default from the
// palette of Qt.
QtObject {
    readonly property SystemPalette palette: SystemPalette {}

    property real gridUnit: 18
    property real smallSpacing: 4
    property real largeSpacing: 8
    property real cornerRadius: 5
    property color highlightColor: palette.highlight
    // Of the window, behind the search bar and the size hint.
    property color backgroundColor: palette.window
    property color textColor: palette.windowText
    property color negativeTextColor: "#da4453"
    // A font of icons, such as Segoe Fluent Icons, for front ends without an icon theme.
    property string iconFont: ""
}
