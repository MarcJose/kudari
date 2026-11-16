// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls

// A setting that is on or off.
FormRow {
    id: root

    property string text
    // The page binds this to its setting. Switching sets it from C++, which keeps that binding,
    // so that the switch follows the setting when it changes elsewhere, as under KDE.
    property alias checked: control.checked
    signal toggled

    caption: text

    Switch {
        id: control

        Accessible.name: root.text
        onToggled: root.toggled()
        // Screen readers and UI Automation switch it without the signal toggled.
        Accessible.onToggleAction: {
            toggle();
            root.toggled();
        }
    }
}
