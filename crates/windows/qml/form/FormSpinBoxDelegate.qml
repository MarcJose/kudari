// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls

// A number, typed or stepped, with an optional unit after it.
FormRow {
    id: root

    property string label
    property alias from: spin.from
    property alias to: spin.to
    property alias stepSize: spin.stepSize
    property alias value: spin.value
    property alias textFromValue: spin.textFromValue
    property alias valueFromText: spin.valueFromText
    property Item trailing

    caption: label
    onTrailingChanged: {
        if (trailing) {
            trailing.parent = unit;
        }
    }

    SpinBox {
        id: spin

        editable: true
        Accessible.name: root.label
    }

    Item {
        id: unit

        implicitWidth: children.length > 0 ? children[0].implicitWidth : 0
        implicitHeight: children.length > 0 ? children[0].implicitHeight : 0
    }
}
