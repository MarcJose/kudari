// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A row of a card: a caption with a description on the left, the control on the right, or
// below the caption with `stacked`.
Item {
    id: root

    property string caption
    property string description
    property bool stacked: false
    default property alias control: holder.data

    Layout.fillWidth: true
    implicitHeight: Math.max(48, layout.implicitHeight + 24)
    opacity: enabled ? 1 : 0.5

    GridLayout {
        id: layout

        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 12
        anchors.bottomMargin: 12
        columns: root.stacked ? 1 : 2
        columnSpacing: 16
        rowSpacing: 8

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            visible: root.caption.length > 0 || root.description.length > 0

            Label {
                Layout.fillWidth: true
                visible: root.caption.length > 0
                text: root.caption
                wrapMode: Text.Wrap
            }

            Label {
                Layout.fillWidth: true
                visible: root.description.length > 0
                text: root.description
                wrapMode: Text.Wrap
                font.pixelSize: 12
                color: palette.placeholderText
            }
        }

        RowLayout {
            id: holder

            Layout.fillWidth: root.stacked
            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
            spacing: 8
        }
    }
}
