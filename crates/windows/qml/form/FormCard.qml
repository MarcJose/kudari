// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Layouts

// A card with settings as its rows.
Rectangle {
    id: root

    default property alias content: rows.data

    Layout.fillWidth: true
    implicitHeight: rows.implicitHeight
    radius: 6
    color: palette.base
    border.color: Qt.alpha(palette.windowText, 0.08)

    ColumnLayout {
        id: rows

        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0
    }
}
