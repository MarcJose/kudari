// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Several lines of text, taken over when the field is left.
FormRow {
    id: root

    property string label
    property alias text: area.text
    signal editingFinished

    caption: label
    stacked: true

    TextArea {
        id: area

        Layout.fillWidth: true
        Layout.minimumHeight: 80
        wrapMode: TextEdit.Wrap
        Accessible.name: root.label
        onEditingFinished: root.editingFinished()
    }
}
