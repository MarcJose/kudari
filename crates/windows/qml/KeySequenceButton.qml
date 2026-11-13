// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls

// Shows a shortcut and takes the next key with its modifiers as the new one after a click.
// Escape gives up.
Button {
    id: root

    required property var controller
    // In the portable text of Qt, several separated by ";", empty for none.
    property string sequence
    property bool recording: false

    signal chosen(string sequence)

    implicitWidth: Math.max(160, implicitContentWidth + leftPadding + rightPadding)
    highlighted: recording
    text: recording ? controller.translate("@action:button", "Press a shortcut…")
        : sequence.trim().length > 0 ? sequence.split(";").map(part => controller.nativeKeySequence(part.trim())).join(", ")
        : controller.translate("@item:inlistbox", "None")
    onClicked: {
        recording = true;
        forceActiveFocus();
    }
    onActiveFocusChanged: {
        if (!activeFocus) {
            recording = false;
        }
    }

    Keys.onPressed: event => {
        if (!recording) {
            return;
        }
        event.accepted = true;
        if (event.key === Qt.Key_Escape) {
            recording = false;
            return;
        }
        const modifiers = [Qt.Key_Shift, Qt.Key_Control, Qt.Key_Alt, Qt.Key_Meta, Qt.Key_AltGr, Qt.Key_unknown];
        if (modifiers.includes(event.key)) {
            return;
        }
        recording = false;
        chosen(controller.keySequenceText(event.key, event.modifiers));
    }
}
