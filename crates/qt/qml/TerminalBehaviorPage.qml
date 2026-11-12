// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import "qrc:/qml/form" as FormCard

FormCard.FormCardPage {
    id: root

    required property var controller
    readonly property var config: JSON.parse(controller.settingsJson).config
    readonly property var bellModes: [
        {text: t("@item:inlistbox", "Notification"), value: "notify"},
        {text: t("@item:inlistbox", "Visual flash"), value: "visual"},
        {text: t("@item:inlistbox", "None"), value: "none"}
    ]
    readonly property var pasteModes: [
        {text: t("@item:inlistbox", "Paste selection"), value: "selection"},
        {text: t("@item:inlistbox", "Paste clipboard"), value: "clipboard"},
        {text: t("@item:inlistbox", "Do nothing"), value: "none"}
    ]

    readonly property var hintModifiers: [
        {text: t("@item:inlistbox", "Off"), value: "none"},
        {text: "Ctrl", value: "ctrl"},
        {text: "Alt", value: "alt"},
        {text: "Meta", value: "meta"},
        {text: "Ctrl+Shift", value: "ctrl+shift"},
        {text: "Ctrl+Alt", value: "ctrl+alt"},
        {text: "Shift+Alt", value: "shift+alt"}
    ]
    readonly property bool hasShells: controller.supports("shells")
    readonly property var shells: [{text: t("@item:inlistbox", "System default"), value: ""}]
        .concat(hasShells ? JSON.parse(controller.shellsJson()) : [])
    readonly property var keyTables: [{text: t("@item:inlistbox", "Built-in"), value: ""}]
        .concat(JSON.parse(controller.keyTablesJson()).map(name => ({text: name, value: name})))
    readonly property var tripleClicks: [
        {text: t("@item:inlistbox", "Select the whole line"), value: "line"},
        {text: t("@item:inlistbox", "Select to the end of the line"), value: "forward"}
    ]
    readonly property var rightClicks: [
        {text: t("@item:inlistbox", "Show the context menu"), value: "none"},
        {text: t("@item:inlistbox", "Paste selection"), value: "selection"},
        {text: t("@item:inlistbox", "Paste clipboard"), value: "clipboard"}
    ]

    function t(context, text) {
        return controller.translate(context, text);
    }

    function set(key, value) {
        controller.setSetting(key, String(value));
    }

    title: t("@title Preferences page name", "Terminal Behavior")

    FormCard.FormHeader {
        title: root.t("@title:group", "Startup")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            visible: root.hasShells
            text: root.t("@label:listbox", "Shell:")
            model: root.shells
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.shells.findIndex(entry => entry.value === root.config.Shell))
            onActivated: root.set("Shell", currentValue)
        }

        FormCard.FormDelegateSeparator {
            visible: root.hasShells
        }

        FormCard.FormSwitchDelegate {
            visible: root.hasShells
            text: root.t("@option:check", "Shell integration")
            description: root.t("@info", "Kudari sets up the prompt of the shell, so that tabs know its directory and AI agents type only at the prompt. Takes effect in new terminals.")
            checked: root.config.ShellIntegration
            onToggled: root.set("ShellIntegration", checked)
        }

        FormCard.FormDelegateSeparator {
            visible: root.hasShells
        }

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Command (empty for the login shell):")
            text: root.config.Command
            onEditingFinished: root.set("Command", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Initial directory (empty for the current directory):")
            text: root.config.Directory
            onEditingFinished: root.set("Directory", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "New terminals start in the directory of the active one")
            description: root.t("@info", "Otherwise they start in the home directory. An initial directory takes precedence.")
            checked: root.config.InheritDirectory
            onToggled: root.set("InheritDirectory", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Title format (%d directory, %n program, %u user, %h host, %w program title):")
            text: root.config.TitleFormat
            onEditingFinished: root.set("TitleFormat", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Title format over SSH (%u user, %U user with @, %h host, %H full host, %c command, %w program title):")
            text: root.config.RemoteTitleFormat
            onEditingFinished: root.set("RemoteTitleFormat", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextAreaDelegate {
            label: root.t("@label:textbox", "Environment variables (one NAME=value per line):")
            text: root.config.Environment
            onEditingFinished: root.set("Environment", text)
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Mouse")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: root.t("@label:listbox", "Middle click:")
            model: root.pasteModes
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.pasteModes.findIndex(entry => entry.value === root.config.MiddleClickPaste))
            onActivated: root.set("MiddleClickPaste", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Copy selected text automatically")
            checked: root.config.AutoCopySelection
            onToggled: root.set("AutoCopySelection", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Change the font size with Ctrl+mouse wheel")
            checked: root.config.WheelZoom
            onToggled: root.set("WheelZoom", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Send arrow keys for the mouse wheel in full-screen programs")
            checked: root.config.AlternateScrolling
            onToggled: root.set("AlternateScrolling", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Characters that end a word on double click (empty for the default):")
            text: root.config.WordSeparators
            onEditingFinished: root.set("WordSeparators", text)
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Selection and Links")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: root.t("@label:listbox", "Triple click:")
            model: root.tripleClicks
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.tripleClicks.findIndex(entry => entry.value === root.config.TripleClick))
            onActivated: root.set("TripleClick", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: root.t("@label:listbox", "Right click:")
            description: root.t("@info", "Shift+right click always shows the context menu.")
            model: root.rightClicks
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.rightClicks.findIndex(entry => entry.value === root.config.RightClickPaste))
            onActivated: root.set("RightClickPaste", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Remove spaces at the start of copied lines")
            checked: root.config.TrimLeadingSpaces
            onToggled: root.set("TrimLeadingSpaces", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Remove spaces at the end of copied lines")
            checked: root.config.TrimTrailingSpaces
            onToggled: root.set("TrimTrailingSpaces", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Clear the selection after copying")
            checked: root.config.ClearSelectionOnCopy
            onToggled: root.set("ClearSelectionOnCopy", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Dragging selected text requires Ctrl")
            checked: root.config.DragNeedsCtrl
            onToggled: root.set("DragNeedsCtrl", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Copy text with colors as HTML as well")
            checked: root.config.CopyAsHtml
            onToggled: root.set("CopyAsHtml", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Underline links under the mouse")
            checked: root.config.UnderlineLinks
            onToggled: root.set("UnderlineLinks", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Underline file names under the mouse")
            checked: root.config.UnderlineFiles
            onToggled: root.set("UnderlineFiles", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Open links with a plain click")
            checked: root.config.OpenLinksByClick
            onToggled: root.set("OpenLinksByClick", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: root.t("@label:listbox", "Hold to number the links on screen:")
            description: root.t("@info", "While holding it, the keys 1 to 9 open the numbered links.")
            model: root.hintModifiers
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.hintModifiers.findIndex(entry => entry.value === root.config.UrlHintsModifier))
            onActivated: root.set("UrlHintsModifier", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Number the links from the bottom")
            checked: root.config.ReverseUrlHints
            onToggled: root.set("ReverseUrlHints", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Preview colors under the mouse")
            checked: root.config.ColorFilter
            onToggled: root.set("ColorFilter", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Ask what to do with dropped files")
            description: root.t("@info", "Offers copying, moving and linking. Otherwise their paths are pasted.")
            checked: !root.config.DropUrlsAsText
            onToggled: root.set("DropUrlsAsText", !checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Link types that may open (separated by semicolons):")
            text: root.config.LinkSchemes
            onEditingFinished: root.set("LinkSchemes", text)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Allow links set by programs")
            description: root.t("@info", "The visible text of such a link can differ from its target.")
            checked: root.config.EscapedLinks
            onToggled: root.set("EscapedLinks", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            label: root.t("@label:textbox", "Editor for file links (PATH, LINE and COLUMN are replaced; empty for the default application):")
            text: root.config.EditorCommand
            onEditingFinished: root.set("EditorCommand", text)
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Notifications")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: root.t("@label:listbox", "Terminal bell:")
            model: root.bellModes
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.bellModes.findIndex(entry => entry.value === root.config.BellMode))
            onActivated: root.set("BellMode", currentValue)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.t("@label:spinbox", "Silence after:")
            from: 1
            to: 3600
            value: root.config.SilenceSeconds
            unit: "s"
            onValueChanged: {
                if (value !== root.config.SilenceSeconds) {
                    root.set("SilenceSeconds", value);
                }
            }
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Display")
    }

    FormCard.FormCard {
        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Dim terminals without focus")
            checked: root.config.DimInactive
            onToggled: root.set("DimInactive", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Mark new lines while scrolling")
            checked: root.config.HighlightScrolledLines
            onToggled: root.set("HighlightScrolledLines", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Show the terminal size after resizing")
            checked: root.config.ShowSizeHint
            onToggled: root.set("ShowSizeHint", checked)
        }
    }

    FormCard.FormHeader {
        title: root.t("@title:group", "Compatibility")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            id: keyBindings

            visible: root.controller.supports("key-tables")
            text: root.t("@label:listbox", "Key bindings:")
            description: root.t("@info", "Konsole's .keytab files from ~/.local/share/kudari/keytabs. They change the keys they name.")
            model: root.keyTables
            textRole: "text"
            valueRole: "value"
            currentIndex: Math.max(0, root.keyTables.findIndex(entry => entry.value === root.config.KeyBindings))
            onActivated: root.set("KeyBindings", currentValue)
        }

        FormCard.FormDelegateSeparator {
            visible: keyBindings.visible
        }

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Programs may use the mouse")
            checked: root.config.MouseTracking
            onToggled: root.set("MouseTracking", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Ctrl+S and Ctrl+Q stop and resume output")
            description: root.t("@info", "Takes effect in new terminals.")
            checked: root.config.FlowControl
            onToggled: root.set("FlowControl", checked)
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Shift+Page Up and Page Down move a full page")
            checked: root.config.ScrollFullPage
            onToggled: root.set("ScrollFullPage", checked)
        }

        FormCard.FormDelegateSeparator {}

        NumberField {
            label: root.t("@label:spinbox", "Auto save interval:")
            from: 1
            to: 3600
            value: root.config.AutoSaveSeconds
            unit: "s"
            onValueChanged: {
                if (value !== root.config.AutoSaveSeconds) {
                    root.set("AutoSaveSeconds", value);
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: root.t("@option:check", "Kitty keyboard protocol")
            description: root.t("@info", "Lets programs tell apart keys that terminals usually mix up. Takes effect in new terminals.")
            checked: root.config.KittyKeyboard
            onToggled: root.set("KittyKeyboard", checked)
        }
    }
}
