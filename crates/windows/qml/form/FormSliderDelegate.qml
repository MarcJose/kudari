// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A value on a scale, with its text beside it.
FormRow {
    id: root

    property string label
    property alias from: slider.from
    property alias to: slider.to
    // Fluent draws a tick for every step, so the slider moves freely and rounds to them.
    property real stepSize: 0
    property alias value: slider.value
    property string valueText

    caption: label

    Slider {
        id: slider

        Layout.preferredWidth: 240
        Accessible.name: root.label
        onMoved: {
            if (root.stepSize > 0) {
                value = Math.round(value / root.stepSize) * root.stepSize;
            }
        }
    }

    Label {
        Layout.minimumWidth: 48
        text: root.valueText
        horizontalAlignment: Text.AlignRight
    }
}
