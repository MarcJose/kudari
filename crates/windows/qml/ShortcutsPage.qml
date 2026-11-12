// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "qrc:/qml/form" as FormCard

// The shortcuts of Windows, where no dialog of KDE changes them: the global one that shows
// and hides the window, and those of the actions inside it.
FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var shortcuts: JSON.parse(controller.shortcutsJson)
    property string toggle: controller.toggleShortcut()
    property bool refused: false
    // The icons of Windows, else text.
    property string iconFont

    function t(context, text) {
        return controller.translate(context, text);
    }

    function same(first, second) {
        return first.join(";") === second.join(";");
    }

    title: t("@title Preferences page name", "Shortcuts")

    FormCard.FormCard {
        Layout.topMargin: 8

        FormCard.FormRow {
            caption: root.controller.actionText("toggle-window-state")
            description: root.refused ? root.t("@info", "Another program already uses this shortcut.") : ""

            KeySequenceButton {
                controller: root.controller
                sequence: root.toggle
                onChosen: sequence => {
                    root.refused = !root.controller.setToggleShortcut(sequence);
                    root.toggle = root.controller.toggleShortcut();
                }
            }
        }
    }

    FormCard.FormCard {
        Layout.topMargin: 16

        Repeater {
            // By number, so that the rows and their focus stay when a shortcut changes.
            model: root.shortcuts.length

            delegate: FormCard.FormRow {
                id: row

                required property int index
                readonly property var modelData: root.shortcuts[index]

                caption: root.controller.actionText(modelData.name)

                KeySequenceButton {
                    controller: root.controller
                    sequence: row.modelData.sequences.join("; ")
                    onChosen: sequence => root.controller.setShortcut(row.modelData.name, sequence)
                }

                ToolButton {
                    enabled: row.modelData.sequences.length > 0
                    text: root.iconFont.length > 0 ? "\uE711" : "✕"
                    font.family: root.iconFont.length > 0 ? root.iconFont : font.family
                    ToolTip.text: root.t("@info:tooltip", "Remove the shortcut")
                    ToolTip.visible: hovered
                    Accessible.name: ToolTip.text
                    onClicked: root.controller.setShortcut(row.modelData.name, "")
                }

                ToolButton {
                    enabled: !root.same(row.modelData.sequences, row.modelData.defaults)
                    text: root.iconFont.length > 0 ? "\uE7A7" : "↺"
                    font.family: root.iconFont.length > 0 ? root.iconFont : font.family
                    ToolTip.text: root.t("@info:tooltip", "Back to the default")
                    ToolTip.visible: hovered
                    Accessible.name: ToolTip.text
                    onClicked: root.controller.setShortcut(row.modelData.name, row.modelData.defaults.join(";"))
                }
            }
        }
    }
}
