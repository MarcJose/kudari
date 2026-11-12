// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A page of settings as Windows 11 shows them: a title above cards, scrolling as a whole.
// The parts of this directory stand in for those of Kirigami Addons, which the settings pages
// of kudari-qt use; they offer what the pages use of them.
ScrollView {
    id: root

    property string title
    default property alias content: column.data

    contentWidth: availableWidth
    clip: true

    ColumnLayout {
        id: column

        x: 24
        width: Math.min(root.availableWidth - 48, 1000)
        spacing: 4

        Label {
            Layout.topMargin: 20
            Layout.bottomMargin: 8
            text: root.title
            font.pixelSize: 28
            font.weight: Font.DemiBold
        }
    }
}
