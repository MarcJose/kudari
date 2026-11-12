// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Layouts
import "qrc:/qml/form" as FormCard

// A color that can be switched off: an empty value means "not set".
ColumnLayout {
    id: root

    required property string label
    required property string value
    property string fallback: "#3daee9"

    signal edited(string value)

    spacing: 0
    Layout.fillWidth: true

    FormCard.FormSwitchDelegate {
        text: root.label
        checked: root.value.length > 0
        onToggled: root.edited(checked ? root.fallback : "")
    }

    FormCard.FormColorDelegate {
        visible: root.value.length > 0
        text: root.label
        color: root.value.length > 0 ? root.value : root.fallback
        onColorChanged: {
            if (String(color) !== root.value && root.value.length > 0) {
                root.edited(String(color));
            }
        }
    }
}
