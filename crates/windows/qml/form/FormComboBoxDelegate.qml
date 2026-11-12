// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A choice from a list.
FormRow {
    id: root

    property string text
    property alias model: box.model
    property alias textRole: box.textRole
    property alias valueRole: box.valueRole
    property alias currentIndex: box.currentIndex
    readonly property alias currentValue: box.currentValue
    signal activated(int index)

    caption: text

    ComboBox {
        id: box

        Layout.preferredWidth: Math.max(200, implicitWidth)
        Accessible.name: root.text
        onActivated: index => root.activated(index)
    }
}
