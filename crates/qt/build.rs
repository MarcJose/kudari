// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

use cxx_qt_build::{CxxQtBuilder, QmlModule};

/// The private headers of `QtGui`, for `QKeyMapper`, and of `QtCore`, which they include,
/// under the headers of the qmake that cxx-qt builds with: `QtGui/<version>/QtGui` and the
/// like.
fn private_gui_headers() -> Vec<std::path::PathBuf> {
    println!("cargo:rerun-if-env-changed=QMAKE");
    let qmakes =
        std::env::var("QMAKE").map_or_else(|_| vec!["qmake6".to_string(), "qmake".to_string()], |qmake| vec![qmake]);
    let query = |qmake: &str, name: &str| {
        std::process::Command::new(qmake)
            .args(["-query", name])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
    };
    qmakes
        .iter()
        .find_map(|qmake| Some((query(qmake, "QT_INSTALL_HEADERS")?, query(qmake, "QT_VERSION")?)))
        .map_or_default(|(headers, version)| {
            ["QtCore", "QtGui"]
                .iter()
                .flat_map(|module| {
                    let private = std::path::Path::new(&headers).join(module).join(&version);
                    [private.clone(), private.join(module)]
                })
                .collect()
        })
}

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
    // SAFETY: the closure only adds an include path and the crate's own source file. It changes none
    // of the settings cxx-qt relies on for the generated code.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            for include in private_gui_headers() {
                cc.include(include);
            }
            cc.file("cpp/qtbridge.cpp");
        })
    };
    builder.build().export();
}
