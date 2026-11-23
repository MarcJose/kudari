// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import org.kde.kirigamiaddons.formcard as Addons

Addons.FormRadioDelegate {
    id: root

    // The row is the radio button, with its name and state. The one Kirigami Addons draws in
    // it takes no focus and has no name: a screen reader would come to a second button there.
    Component.onCompleted: {
        const items = [root.contentItem];
        while (items.length > 0) {
            const item = items.shift();
            if (item.objectName === "checkControl") {
                item.Accessible.ignored = true;
                return;
            }
            items.push(...item.children);
        }
    }
}
