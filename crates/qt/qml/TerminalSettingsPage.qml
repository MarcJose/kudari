// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config
    readonly property string systemDefault: controller.translate("@item:inlistbox", "System default")
    readonly property var fonts: [{text: systemDefault, value: ""}].concat(JSON.parse(controller.fixedFontsJson()).map(family => ({text: family, value: family})))
    readonly property var scrollBars: [
        {text: controller.translate("@item:inlistbox", "Right"), value: "right"},
        {text: controller.translate("@item:inlistbox", "Left"), value: "left"},
        {text: controller.translate("@item:inlistbox", "Hidden"), value: "hidden"}
    ]
    readonly property var cursors: [
        {text: controller.translate("@item:inlistbox", "Block"), value: "block"},
        {text: controller.translate("@item:inlistbox", "Underline"), value: "underline"},
        {text: controller.translate("@item:inlistbox", "I-Beam"), value: "beam"}
    ]

    function set(key, value) {
        controller.setSetting(key, String(value));
    }

    title: controller.translate("@title Preferences page name", "Terminal")

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Font")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: root.controller.translate("@label:listbox", "Font family:")
            model: root.fonts
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.fonts.findIndex(font => font.value === root.config.FontFamily))
            onActivated: root.set("FontFamily", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Font size:")
            from: 0
            to: 72
            value: root.config.FontSize
            unit: "pt"
            zeroText: root.systemDefault
            onValueChanged: {
                if (value !== root.config.FontSize) {
                    root.set("FontSize", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Line spacing:")
            from: 0
            to: 20
            value: root.config.LineSpacing
            unit: "px"
            onValueChanged: {
                if (value !== root.config.LineSpacing) {
                    root.set("LineSpacing", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Draw bold text in bright colors")
            checked: root.config.BoldIntense
            onToggled: root.set("BoldIntense", checked)
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Layout")
    }

    FormCard.FormCard {
        NumberField {
            label: root.controller.translate("@label:spinbox", "Margin around the text:")
            from: 0
            to: 50
            value: root.config.Margin
            unit: "px"
            onValueChanged: {
                if (value !== root.config.Margin) {
                    root.set("Margin", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Center the text")
            checked: root.config.CenterContent
            onToggled: root.set("CenterContent", checked)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Vertical line at column:")
            from: 0
            to: 500
            value: root.config.VerticalLine
            zeroText: root.controller.translate("@item:inrange", "Off")
            onValueChanged: {
                if (value !== root.config.VerticalLine) {
                    root.set("VerticalLine", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Show line numbers")
            checked: root.config.LineNumbers
            onToggled: root.set("LineNumbers", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.controller.translate("@label:textbox", "Emoji font (empty for automatic):")
            text: root.config.EmojiFont
            onEditingFinished: root.set("EmojiFont", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Use the font for line and block characters")
            description: root.controller.translate("@info", "Otherwise they are drawn without gaps between the cells.")
            checked: root.config.FontLineCharacters
            onToggled: root.set("FontLineCharacters", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Use the font for Braille characters")
            checked: root.config.FontBrailleCharacters
            onToggled: root.set("FontBrailleCharacters", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Smooth fonts")
            checked: root.config.AntiAliasFonts
            onToggled: root.set("AntiAliasFonts", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Full font hinting")
            checked: root.config.FontHinting
            onToggled: root.set("FontHinting", checked)
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Focus")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Fade the blinking cursor")
            checked: root.config.AnimatedCursor
            onToggled: root.set("AnimatedCursor", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Invert the colors of selected text")
            checked: root.config.InvertSelection
            onToggled: root.set("InvertSelection", checked)
        }

        FormCard.FormDelegateSeparator {}

        OptionalColor {
            label: root.controller.translate("@option:check", "Cursor color")
            value: root.config.CursorColor
            onEdited: value => root.set("CursorColor", value)
        }

        FormCard.FormDelegateSeparator {}

        OptionalColor {
            label: root.controller.translate("@option:check", "Text color under the cursor")
            value: root.config.CursorTextColor
            fallback: "#000000"
            onEdited: value => root.set("CursorTextColor", value)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Border around the terminal with focus")
            checked: root.config.BorderWhenActive
            onToggled: root.set("BorderWhenActive", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: root.controller.translate("@label:chooser", "Border color:")
            color: root.config.FocusBorderColor
            enabled: root.config.BorderWhenActive
            onColorChanged: {
                if (String(color) !== root.config.FocusBorderColor) {
                    root.set("FocusBorderColor", String(color));
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Dimming of terminals without focus:")
            from: 0
            to: 255
            value: root.config.DimValue
            onValueChanged: {
                if (value !== root.config.DimValue) {
                    root.set("DimValue", value);
                }
            }
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Tabs")
    }

    FormCard.FormCard {
        OptionalColor {
            label: root.controller.translate("@option:check", "Tab color")
            value: root.config.TabColor
            onEdited: value => root.set("TabColor", value)
        }

        FormCard.FormDelegateSeparator {}

        OptionalColor {
            label: root.controller.translate("@option:check", "Tab title color after output in the background")
            value: root.config.TabActivityColor
            fallback: "#da4453"
            onEdited: value => root.set("TabActivityColor", value)
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Scrollbar Marks")
    }

    FormCard.FormCard {
        FormCard.FormColorDelegate {
            text: root.controller.translate("@label:chooser", "Color of search matches:")
            color: root.config.SearchLineColor
            onColorChanged: {
                if (String(color) !== root.config.SearchLineColor) {
                    root.set("SearchLineColor", String(color));
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Opacity of search matches:")
            from: 0
            to: 100
            value: root.config.SearchLineOpacity
            unit: "%"
            onValueChanged: {
                if (value !== root.config.SearchLineOpacity) {
                    root.set("SearchLineOpacity", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: root.controller.translate("@info", "A double click on the scrollbar adds or removes a marker.")
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: root.controller.translate("@label:chooser", "Color of markers:")
            color: root.config.MarkerColor
            onColorChanged: {
                if (String(color) !== root.config.MarkerColor) {
                    root.set("MarkerColor", String(color));
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Size of markers:")
            from: 1
            to: 50
            value: root.config.MarkerSize
            unit: "%"
            onValueChanged: {
                if (value !== root.config.MarkerSize) {
                    root.set("MarkerSize", value);
                }
            }
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Badge")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Show a badge in the corner of the terminal")
            checked: root.config.BadgeEnabled
            onToggled: root.set("BadgeEnabled", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.controller.translate("@label:textbox", "Badge text (placeholders as in the title format):")
            text: root.config.BadgeText
            enabled: root.config.BadgeEnabled
            onEditingFinished: root.set("BadgeText", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: root.controller.translate("@label:chooser", "Badge color:")
            color: root.config.BadgeColor
            enabled: root.config.BadgeEnabled
            onColorChanged: {
                if (String(color) !== root.config.BadgeColor) {
                    root.set("BadgeColor", String(color));
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.controller.translate("@label:textbox", "Badge font (empty for the terminal font):")
            text: root.config.BadgeFontFamily
            enabled: root.config.BadgeEnabled
            onEditingFinished: root.set("BadgeFontFamily", text)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Badge font size:")
            from: 0
            to: 200
            value: root.config.BadgeFontSize
            enabled: root.config.BadgeEnabled
            unit: "pt"
            zeroText: root.systemDefault
            onValueChanged: {
                if (value !== root.config.BadgeFontSize) {
                    root.set("BadgeFontSize", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Only the text, without a background")
            checked: root.config.BadgeTextOnly
            enabled: root.config.BadgeEnabled
            onToggled: root.set("BadgeTextOnly", checked)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.controller.translate("@label:spinbox", "Badge opacity:")
            from: 0
            to: 255
            value: root.config.BadgeTransparency
            enabled: root.config.BadgeEnabled
            onValueChanged: {
                if (value !== root.config.BadgeTransparency) {
                    root.set("BadgeTransparency", value);
                }
            }
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Scrolling and Cursor")
    }

    FormCard.FormCard {
        NumberField {
            label: root.controller.translate("@label:spinbox", "History lines:")
            from: 0
            to: 1000000
            stepSize: 1000
            value: root.config.HistoryLines
            onValueChanged: {
                if (value !== root.config.HistoryLines) {
                    root.set("HistoryLines", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: root.controller.translate("@label:listbox", "Scrollbar:")
            model: root.scrollBars
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.scrollBars.findIndex(entry => entry.value === root.config.ScrollBarPosition))
            onActivated: root.set("ScrollBarPosition", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Blinking cursor")
            checked: root.config.BlinkingCursor
            onToggled: root.set("BlinkingCursor", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: root.controller.translate("@label:listbox", "Cursor shape:")
            model: root.cursors
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.cursors.findIndex(cursor => cursor.value === root.config.CursorShape))
            onActivated: root.set("CursorShape", currentValue)
        }
    }

    FormCard.FormHeader {
        title: root.controller.translate("@title:group", "Transparency")
    }

    FormCard.FormCard {
        NumberField {
            label: root.controller.translate("@label:spinbox", "Background opacity:")
            from: 10
            to: 100
            stepSize: 5
            value: root.config.Opacity
            unit: "%"
            onValueChanged: {
                if (value !== root.config.Opacity) {
                    root.set("Opacity", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.controller.translate("@option:check", "Blur the background")
            enabled: root.config.Opacity < 100
            checked: root.config.Blur
            onToggled: root.set("Blur", checked)
        }
    }
}
