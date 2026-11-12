// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Layouts

// The line between two rows of a card.
Rectangle {
    Layout.fillWidth: true
    implicitHeight: 1
    color: Qt.alpha(palette.windowText, 0.08)
}
