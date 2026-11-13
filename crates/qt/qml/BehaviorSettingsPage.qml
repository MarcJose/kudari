// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config
    // Two of these have no context in the translations, as in Yakuake.
    readonly property var general: [
        {key: "KeepOpen", text: controller.translate("@option:check", "Keep the window open when another window gets the focus")},
        {key: "KeepOpenAfterLastSessionCloses", text: controller.translate("@option:check", "Keep the window open when its last session closes")},
        {key: "ToggleToFocus", text: controller.translate("@option:check", "Let the show/hide shortcut focus the window instead of hiding it")},
        {key: "OpenAfterStart", text: controller.translate("@option:check", "Show the window when Kudari starts")},
        {key: "FocusFollowsMouse", text: controller.translate("@option:check", "Focus the terminal under the mouse pointer")},
        {key: "RememberFullscreen", text: controller.translate("", "Come back in full screen if the window was in full screen")},
        {key: "ShowSystrayIcon", text: controller.translate("", "Show an icon in the system tray")}
    ]

    title: controller.translate("@title Preferences page name", "Behavior")

    FormCard.FormHeader {
        title: root.controller.translate("@title:group Group box label", "General")
    }

    FormCard.FormCard {
        Repeater {
            model: root.general

            delegate: FormCard.FormSwitchDelegate {
                required property var modelData

                text: modelData.text
                checked: root.config[modelData.key]
                onToggled: root.controller.setSetting(modelData.key, String(checked))
            }
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Questions")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Ask before quitting with several sessions open")
            checked: root.config.ConfirmQuit
            onToggled: root.controller.setSetting("ConfirmQuit", String(checked))
        }
    }

    FormCard.FormHeader {
        visible: shortcutsButton.visible || importButton.visible
        title: root.controller.translate("@title:menu", "Settings")
    }

    FormCard.FormCard {
        visible: shortcutsButton.visible || importButton.visible

        FormCard.FormButtonDelegate {
            id: shortcutsButton

            visible: root.controller.supports("shortcut-dialog")
            icon.name: "configure-shortcuts"
            text: root.controller.actionText("options_configure_keybinding")
            onClicked: root.controller.trigger("options_configure_keybinding")
        }

        FormCard.FormDelegateSeparator {
            visible: shortcutsButton.visible && importButton.visible
        }

        FormCard.FormButtonDelegate {
            id: importButton

            visible: root.controller.canImportYakuake()
            icon.name: "document-import"
            text: root.controller.translate("@action:button", "Import from Yakuake…")
            onClicked: root.controller.askImport()
        }
    }
}
