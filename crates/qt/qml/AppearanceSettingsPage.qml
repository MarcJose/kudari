// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

pragma ComponentBehavior: Bound

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config
    readonly property var skins: JSON.parse(controller.availableSkinsJson())
    // Themes, border and background belong to the window of KDE.
    readonly property bool hasSkins: controller.supports("skins")
    // Where there are no themes, light or dark is a choice of its own.
    readonly property bool hasColorScheme: controller.supports("color-scheme")
    readonly property var colorSchemes: [
        {text: t("@item:inlistbox", "System default"), value: "system"},
        {text: t("@item:inlistbox", "Light"), value: "light"},
        {text: t("@item:inlistbox", "Dark"), value: "dark"}
    ]

    function t(context, text) {
        return controller.translate(context, text);
    }

    title: controller.translate("@title Preferences page name", "Appearance")

    FormCard.FormHeader {
        visible: root.hasSkins
        title: root.t("@title:group", "Theme")
    }

    FormCard.FormCard {
        visible: root.hasSkins

        Repeater {
            model: root.skins

            delegate: FormCard.FormRadioDelegate {
                required property var modelData

                text: modelData.name
                description: modelData.author
                checked: modelData.id === root.config.Skin
                onToggled: root.controller.setSetting("Skin", modelData.id)

            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("", "Hide the window border")
            checked: root.config.HideSkinBorders
            onToggled: root.controller.setSetting("HideSkinBorders", String(checked))
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group Group box label", "General")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            visible: root.hasColorScheme
            text: root.t("@label:listbox", "Color scheme:")
            model: root.colorSchemes
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.colorSchemes.findIndex(entry => entry.value === root.config.ColorScheme))
            onActivated: root.controller.setSetting("ColorScheme", currentValue)
        }

        FormCard.FormDelegateSeparator {
            visible: root.hasColorScheme
        }

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Flash a terminal briefly when it gets the focus")
            checked: root.config.TerminalHighlightOnManualActivation
            onToggled: root.controller.setSetting("TerminalHighlightOnManualActivation", String(checked))
        }
    }

    FormCard.FormHeader {
        visible: root.hasSkins
        title: root.t("@title:group", "Background")
    }

    FormCard.FormCard {
        visible: root.hasSkins

        FormCard.FormColorDelegate {
            text: root.t("@label:chooser", "Background color:")
            color: root.config.BackgroundColor
            onColorChanged: {
                if (String(color) !== root.config.BackgroundColor) {
                    root.controller.setSetting("BackgroundColor", String(color));
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Let the desktop show through the background")
            checked: root.config.Translucency
            onToggled: root.controller.setSetting("Translucency", String(checked))
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.t("@label:spinbox", "Opacity of the background color:")
            from: 0
            to: 100
            value: root.config.BackgroundColorOpacity
            enabled: root.config.Translucency
            unit: "%"
            onValueChanged: {
                if (value !== root.config.BackgroundColorOpacity) {
                    root.controller.setSetting("BackgroundColorOpacity", String(value));
                }
            }
        }
    }
}
