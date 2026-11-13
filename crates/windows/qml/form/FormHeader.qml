// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The title of a group of settings.
Label {
    property string title

    Layout.fillWidth: true
    Layout.topMargin: 20
    Layout.bottomMargin: 4
    text: title
    font.weight: Font.DemiBold
    elide: Text.ElideRight
}
