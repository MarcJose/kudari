// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs

// A color, chosen in the color dialog.
FormRow {
    id: root

    property string text
    property color color

    caption: text

    Button {
        Accessible.name: root.text
        implicitWidth: 64
        onClicked: dialog.open()

        contentItem: Rectangle {
            implicitHeight: 16
            radius: 3
            color: root.color
            border.color: Qt.alpha(palette.windowText, 0.3)
        }
    }

    ColorDialog {
        id: dialog

        title: root.text
        selectedColor: root.color
        onAccepted: root.color = selectedColor
    }
}
