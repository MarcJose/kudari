// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlModule};

/// KDE Frameworks without Rust bindings, used by `cpp/kdebridge.cpp`.
const KF6_MODULES: [(&str, &str); 22] = [
    ("KIO", "KF6KIOCore"),
    ("KIOCore", "KF6KIOCore"),
    ("KIOGui", "KF6KIOGui"),
    ("KIOWidgets", "KF6KIOCore"),
    ("KJobWidgets", "KF6JobWidgets"),
    ("KService", "KF6Service"),
    ("KCrash", "KF6Crash"),
    ("KConfig", "KF6ConfigCore"),
    ("KConfigCore", "KF6ConfigCore"),
    ("KConfigGui", "KF6ConfigGui"),
    ("KConfigWidgets", "KF6ConfigWidgets"),
    ("KCoreAddons", "KF6CoreAddons"),
    ("KGlobalAccel", "KF6GlobalAccel"),
    ("KI18n", "KF6I18n"),
    ("KI18n", "KF6I18nQml"),
    ("KNotifications", "KF6Notifications"),
    ("KNotifyConfig", "KF6NotifyConfig"),
    ("KStatusNotifierItem", "KF6StatusNotifierItem"),
    ("KWidgetsAddons", "KF6WidgetsAddons"),
    ("KWindowSystem", "KF6WindowSystem"),
    ("KXmlGui", "KF6XmlGui"),
    ("KColorScheme", "KF6ColorScheme"),
];

fn main() {
    println!("cargo:rerun-if-changed=cpp/kdebridge.cpp");
    println!("cargo:rerun-if-changed=cpp/kdebridge.h");

    let qml = [
        "qml/FirstRunDialog.qml",
        "qml/Main.qml",
        "qml/TabBar.qml",
        "qml/WindowButtons.qml",
    ];
    let mut module = QmlModule::new("io.github.marcjose.kudari");
    for file in qml {
        module = module.qml_file(file);
    }

    // The parts of the settings pages of kudari-qt, from Kirigami Addons: they import the
    // directory qrc:/qml/form, which each front end fills with its own.
    println!("cargo:rerun-if-changed=qml/form");
    let form = std::fs::read_dir("qml/form")
        .map(|entries| {
            let mut files: Vec<_> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
            files.sort();
            files
        })
        .unwrap_or_default();
    let builder = CxxQtBuilder::new_qml_module(module)
        .qrc_resources(QResources::new().resource(QResource::new().prefix("/").files(form)))
        .files(["src/kde.rs"])
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Widgets")
        .qt_module("PrintSupport")
        .qt_module("DBus")
        .qt_module("Network");
    // SAFETY: the closure only adds include paths, flags and our own source file. It
    // changes none of the settings cxx-qt relies on for the generated code.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            cc.include("/usr/include/qt6/QtQmlIntegration");
            cc.flag_if_supported("-Wno-unused-parameter");
            for (include, _) in KF6_MODULES {
                cc.include(format!("/usr/include/KF6/{include}"));
            }
            cc.file("cpp/kdebridge.cpp");
            if std::env::var_os("CARGO_FEATURE_TEST_HOOKS").is_some() {
                cc.define("RK_TEST_HOOKS", None);
                cc.define("QT_GUI_LIB", None);
                cc.include("/usr/include/qt6/QtTest");
            }
        })
    };
    if std::env::var_os("CARGO_FEATURE_TEST_HOOKS").is_some() {
        println!("cargo:rustc-link-lib=Qt6Test");
    }
    for (_, library) in KF6_MODULES {
        println!("cargo:rustc-link-lib={library}");
    }
    // KIOWidgets registers the dialogs of KIO jobs when loaded; no symbol refers to it.
    println!("cargo:rustc-link-arg=-Wl,--no-as-needed,-lKF6KIOWidgets,--as-needed");
    builder.build();
}
