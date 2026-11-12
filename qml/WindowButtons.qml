// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The buttons of the window: the access of AI agents, keep it open, the menu, and quit.
RowLayout {
    id: buttons

    required property var controller
    required property bool keepOpen
    property int buttonSize: Kirigami.Units.iconSizes.small + 2 * Kirigami.Units.smallSpacing

    spacing: 0

    component WindowButton: QQC2.ToolButton {
        Layout.preferredWidth: buttons.buttonSize
        Layout.preferredHeight: buttons.buttonSize
        Layout.alignment: Qt.AlignVCenter
        icon.width: Kirigami.Units.iconSizes.small
        icon.height: Kirigami.Units.iconSizes.small
        display: QQC2.AbstractButton.IconOnly
        QQC2.ToolTip.text: text
        QQC2.ToolTip.visible: hovered
        QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
    }

    // Shows while an AI agent may use Kudari; a click ends its access.
    WindowButton {
        visible: buttons.controller.agentAccess
        icon.name: "view-visible"
        text: buttons.controller.translate("@info:tooltip", "An AI agent has access to your terminals. Click to end its access.")
        onClicked: buttons.controller.revokeAgentAccess()
    }

    WindowButton {
        icon.name: "window-pin"
        checkable: true
        checked: buttons.keepOpen
        text: buttons.controller.translate("@info:tooltip", "Keep open when the focus moves elsewhere")
        onClicked: buttons.controller.setKeepOpen(checked)
    }

    WindowButton {
        icon.name: "application-menu"
        text: buttons.controller.translate("@info:tooltip", "Open Menu")
        onClicked: {
            const point = mapToItem(null, 0, height);
            buttons.controller.popupMainMenu(point.x, point.y);
        }
    }

    WindowButton {
        icon.name: "window-close"
        text: buttons.controller.translate("@info:tooltip Quits the application", "Quit")
        onClicked: buttons.controller.trigger("file_quit")
    }
}
