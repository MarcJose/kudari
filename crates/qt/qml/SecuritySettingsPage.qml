// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config

    title: controller.translate("@title Preferences page name", "Security")

    FormCard.FormHeader {
        visible: root.controller.supports("remote-commands")
        title: root.controller.translate("@title:group", "Remote Control")
    }

    FormCard.FormCard {
        visible: root.controller.supports("remote-commands")

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Allow other programs to run commands")
            description: root.controller.translate("@info", "Lets the D-Bus methods runCommand and runCommandInTerminal type into your terminals. Every program running as your user could then execute commands through them. Leave this off unless a script of yours needs it.")
            checked: root.config.AllowRemoteCommands
            onToggled: root.controller.setSetting("AllowRemoteCommands", String(checked))
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "AI Agents")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Allow AI agents to read terminals")
            description: root.controller.translate("@info", "Agents connected through kudari mcp see the text of all terminals, including passwords or tokens shown there.")
            checked: root.config.AllowAgentReading
            onToggled: root.controller.setSetting("AllowAgentReading", String(checked))
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            id: agentControl
            text: root.controller.translate("@option:check", "Allow AI agents to control terminals")
            description: root.controller.translate("@info", "Agents can type into terminals, press keys, and open and close sessions of their own.")
            checked: root.config.AllowAgentControl
            onToggled: root.controller.setSetting("AllowAgentControl", String(checked))
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Ask before running their commands")
            description: root.controller.translate("@info", "Agents type only at the shell prompt, and their commands run when you press Return.")
            enabled: agentControl.checked
            checked: root.config.ConfirmAgentCommands
            onToggled: root.controller.setSetting("ConfirmAgentCommands", String(checked))
        }
    }
}
