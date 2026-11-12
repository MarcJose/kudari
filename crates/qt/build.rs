// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    println!("cargo:rerun-if-changed=cpp/qtbridge.cpp");
    println!("cargo:rerun-if-changed=cpp/qtbridge.h");

    // The settings pages import the directory qrc:/qml/form, where each front end puts the
    // parts of its forms.
    let module = QmlModule::new("io.github.marcjose.kudari.qt").qml_files([
        "qml/AppearanceSettingsPage.qml",
        "qml/BehaviorSettingsPage.qml",
        "qml/NumberField.qml",
        "qml/OptionalColor.qml",
        "qml/PaneTheme.qml",
        "qml/SecuritySettingsPage.qml",
        "qml/SessionArea.qml",
        "qml/TerminalBehaviorPage.qml",
        "qml/TerminalPane.qml",
        "qml/TerminalSettingsPage.qml",
        "qml/WindowSettingsPage.qml",
    ]);
    let builder = CxxQtBuilder::new_qml_module(module)
        .files(["src/controller.rs", "src/qt.rs", "src/terminal_item.rs"])
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Widgets")
        .qt_module("PrintSupport");
    // SAFETY: the closure only adds an include path and our own source file. It changes none
    // of the settings cxx-qt relies on for the generated code.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            cc.file("cpp/qtbridge.cpp");
        })
    };
    builder.build().export();
}
