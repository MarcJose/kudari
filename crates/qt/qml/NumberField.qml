// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import "qrc:/qml/form" as FormCard

// A whole number. The unit stays outside of the field: the spin box reads its text back when
// it loses focus, and its validator would reject any typing next to a unit.
FormCard.FormSpinBoxDelegate {
    id: root

    property string unit
    // What the field shows for 0, if that value means something special.
    property string zeroText

    textFromValue: value => value === 0 && zeroText.length > 0 ? zeroText : String(value)
    // Anything else in the field, like nothing at all, leaves the value alone.
    valueFromText: (text, locale) => {
        try {
            return text.length > 0 ? Number.fromLocaleString(locale, text) : value;
        } catch (error) {
            return value;
        }
    }

    trailing: QQC2.Label {
        text: root.value === 0 && root.zeroText.length > 0 ? "" : root.unit
    }
}
