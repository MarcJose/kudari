// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

import QtQuick
import org.kde.kirigamiaddons.formcard as Addons

Addons.FormTextFieldDelegate {
    // Kirigami Addons makes the row a text field of its own as well, without a name: a
    // screen reader that comes to it says which field it is.
    Accessible.name: label
}
