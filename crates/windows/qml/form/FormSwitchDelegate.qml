// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls

// A setting that is on or off.
FormRow {
    id: root

    property string text
    property bool checked
    signal toggled

    caption: text

    // Any change that does not come from the page counts: screen readers and UI Automation
    // switch it without the signal toggled.
    Switch {
        checked: root.checked
        Accessible.name: root.text
        onCheckedChanged: {
            if (checked !== root.checked) {
                root.checked = checked;
                root.toggled();
            }
        }
    }
}
