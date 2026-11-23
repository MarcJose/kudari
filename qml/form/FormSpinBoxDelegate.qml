// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import org.kde.kirigamiaddons.formcard as Addons

Addons.FormSpinBoxDelegate {
    id: root

    // Kirigami Addons gives the spin box in it no name, so a screen reader announced a bare
    // number: it gets the label of the row, as the one of Windows has it.
    Component.onCompleted: {
        const boxes = [root.contentItem];
        while (boxes.length > 0) {
            const item = boxes.shift();
            if (item.up !== undefined && item.stepSize !== undefined) {
                item.Accessible.name = Qt.binding(() => root.label);
                return;
            }
            boxes.push(...item.children);
        }
    }
}
