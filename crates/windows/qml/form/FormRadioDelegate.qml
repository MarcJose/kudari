// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls

// One of several choices; the page decides which one is checked.
FormRow {
    id: root

    property string text
    property bool checked
    signal toggled

    caption: text

    RadioButton {
        checked: root.checked
        Accessible.name: root.text
        onClicked: {
            root.toggled();
            checked = Qt.binding(() => root.checked);
        }
    }
}
