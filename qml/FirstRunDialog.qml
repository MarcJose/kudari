// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kquickcontrols as KQuickControls

// Welcomes new users, asks for the global shortcut and offers the Yakuake import.
QQC2.ApplicationWindow {
    id: dialog

    required property var controller
    readonly property bool canImport: controller.canImportYakuake()
    readonly property string currentShortcut: controller.toggleShortcut()
    readonly property string yakuakeShortcut: controller.yakuakeShortcut()
    property bool done: false

    signal finished

    function finish(accepted) {
        if (done) {
            return;
        }
        done = true;
        controller.finishFirstRun(accepted && importSwitch.visible && importSwitch.checked, accepted ? sequenceText.portableText : "");
        finished();
    }

    title: controller.translate("@title:window", "Welcome")
    flags: Qt.Dialog
    visible: true
    width: Kirigami.Units.gridUnit * 28
    height: content.implicitHeight + footer.implicitHeight + Kirigami.Units.gridUnit * 2
    onClosing: finish(false)

    // Turns the sequence of the input field into text.
    Shortcut {
        id: sequenceText

        enabled: false
        sequence: hotkey.keySequence
    }

    Shortcut {
        sequence: "Ctrl+Return"
        onActivated: dialog.finish(true)
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: Kirigami.Units.gridUnit
        spacing: Kirigami.Units.largeSpacing

        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "io.github.marcjose.kudari"
                fallback: "utilities-terminal"
                Layout.preferredWidth: Kirigami.Units.iconSizes.large
                Layout.preferredHeight: Kirigami.Units.iconSizes.large
            }

            Kirigami.Heading {
                text: dialog.controller.translate("@title", "Welcome to Kudari")
                Layout.fillWidth: true
            }
        }

        QQC2.Label {
            text: dialog.controller.translate("@info", "Kudari slides down and up again with a keyboard shortcut. You can pick another one now:")
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        KQuickControls.KeySequenceItem {
            id: hotkey

            // Conflicts are resolved with KDE's own prompt when the dialog is accepted.
            checkForConflictsAgainst: 0
            multiKeyShortcutsAllowed: false
            patterns: KQuickControls.ShortcutPattern.Key | KQuickControls.ShortcutPattern.ModifierAndKey
            keySequence: dialog.currentShortcut
            Layout.alignment: Qt.AlignHCenter
        }

        QQC2.Label {
            visible: dialog.yakuakeShortcut.length > 0
            text: dialog.controller.translateWith("@info", "Yakuake is using %1. If you choose the same key, Kudari will ask before taking it over.", dialog.yakuakeShortcut)
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        QQC2.Label {
            text: dialog.controller.translate("@info", "The menu lets you change it later as well.")
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        QQC2.Switch {
            id: importSwitch

            visible: dialog.canImport
            checked: true
            text: dialog.controller.translate("@option:check", "Take over settings from Yakuake")
            Layout.fillWidth: true
        }

        QQC2.Label {
            visible: dialog.canImport
            text: dialog.controller.translate("@info", "Window size and keyboard shortcuts, plus font and colors of your Konsole profile. These settings are only read, not changed.")
            wrapMode: Text.WordWrap
            font: Kirigami.Theme.smallFont
            opacity: 0.8
            Layout.fillWidth: true
        }

        Item {
            Layout.fillHeight: true
        }
    }

    footer: QQC2.DialogButtonBox {
        standardButtons: QQC2.DialogButtonBox.Ok | QQC2.DialogButtonBox.Cancel
        onAccepted: dialog.finish(true)
        onRejected: dialog.finish(false)
    }
}
