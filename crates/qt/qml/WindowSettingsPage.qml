// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config
    readonly property var screens: {
        const names = controller.outputNames().split(",");
        return [{text: controller.translate("@item:inmenu", "Where the mouse pointer is"), value: 0}]
            .concat(names.map((name, index) => ({text: name, value: index + 1})));
    }

    function t(context, text) {
        return controller.translate(context, text);
    }

    function set(key, value) {
        controller.setSetting(key, String(value));
    }

    title: controller.translate("@title Preferences page name", "Window")

    FormCard.FormHeader {
        title: root.t("@title:group", "Size")
    }

    FormCard.FormCard {
        NumberField {
            label: root.t("@label:slider", "Width:")
            from: 10
            to: 100
            value: root.config.Width
            unit: "%"
            onValueChanged: {
                if (value !== root.config.Width) {
                    root.set("Width", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.t("@label:slider", "Height:")
            from: 10
            to: 100
            value: root.config.Height
            unit: "%"
            onValueChanged: {
                if (value !== root.config.Height) {
                    root.set("Height", value);
                }
            }
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Position")
    }

    FormCard.FormCard {
        FormCard.FormSliderDelegate {
            label: root.t("@item:inrange Direction to move the window in", "Left") + " ↔ " + root.t("@item:inrange Direction to move the window in", "Right")
            from: 0
            to: 100
            stepSize: 1
            value: root.config.Position
            valueText: Math.round(value) + " %"
            onValueChanged: {
                if (Math.round(value) !== root.config.Position) {
                    root.set("Position", Math.round(value));
                }
            }
        }

        FormCard.FormDelegateSeparator {
            visible: screenChoice.visible
        }

        FormCard.FormComboBoxDelegate {
            id: screenChoice

            visible: root.screens.length > 2
            text: root.t("@label:listbox", "Screen:")
            model: root.screens
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.screens.findIndex(screen => screen.value === root.config.Screen))
            onActivated: root.set("Screen", currentValue)
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Tab Bar")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Show the tab bar")
            checked: root.config.ShowTabBar
            onToggled: root.set("ShowTabBar", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Name tabs after the title of their terminal")
            checked: root.config.DynamicTabTitles
            onToggled: root.set("DynamicTabTitles", checked)
        }
    }
}
