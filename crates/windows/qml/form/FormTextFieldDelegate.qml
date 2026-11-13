// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A line of text, taken over when the field is left or Return is pressed.
FormRow {
    id: root

    property string label
    property alias placeholderText: field.placeholderText
    property alias text: field.text
    signal editingFinished

    caption: label
    stacked: true

    TextField {
        id: field

        Layout.fillWidth: true
        Accessible.name: root.label
        onEditingFinished: root.editingFinished()
    }
}
