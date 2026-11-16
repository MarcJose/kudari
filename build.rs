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
    // KDE Frameworks 6.32 moved the headers of KI18nQml, such as KLocalizedQmlContext, out of
    // KI18n into a folder of their own; older versions keep them in KI18n.
    ("KI18nQml", "KF6I18nQml"),
    ("KNotifications", "KF6Notifications"),
    ("KNotifyConfig", "KF6NotifyConfig"),
    ("KStatusNotifierItem", "KF6StatusNotifierItem"),
    ("KWidgetsAddons", "KF6WidgetsAddons"),
    ("KWindowSystem", "KF6WindowSystem"),
    ("KXmlGui", "KF6XmlGui"),
    ("KColorScheme", "KF6ColorScheme"),
];

/// The headers of Qt, from the qmake that cxx-qt builds with as well: `QMAKE`, or `qmake6` or
/// `qmake` on the path.
fn qt_headers() -> std::path::PathBuf {
    println!("cargo:rerun-if-env-changed=QMAKE");
    let qmakes =
        std::env::var("QMAKE").map_or_else(|_| vec!["qmake6".to_string(), "qmake".to_string()], |qmake| vec![qmake]);
    qmakes
        .iter()
        .filter_map(|qmake| {
            std::process::Command::new(qmake)
                .args(["-query", "QT_INSTALL_HEADERS"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .find(|headers| !headers.is_empty())
        .map_or_else(|| "/usr/include/qt6".into(), Into::into)
}

/// Where the KDE Frameworks are installed: `KUDARI_KF6_PREFIX`, which the build with `cmake`
/// sets to the one it found, else the first entry of `CMAKE_PREFIX_PATH` with them, as for
/// frameworks of one's own built with kde-builder, else `/usr`.
fn kf6_prefix() -> std::path::PathBuf {
    println!("cargo:rerun-if-env-changed=KUDARI_KF6_PREFIX");
    println!("cargo:rerun-if-env-changed=CMAKE_PREFIX_PATH");
    if let Some(prefix) = std::env::var_os("KUDARI_KF6_PREFIX").filter(|prefix| !prefix.is_empty()) {
        return prefix.into();
    }
    std::env::var_os("CMAKE_PREFIX_PATH")
        .and_then(|paths| std::env::split_paths(&paths).find(|prefix| prefix.join("include/KF6/KI18n").is_dir()))
        .unwrap_or_else(|| "/usr".into())
}

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
    let form = std::fs::read_dir("qml/form").map_or_default(|entries| {
        let mut files: Vec<_> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
        files.sort();
        files
    });
    let test_hooks = std::env::var_os("CARGO_FEATURE_TEST_HOOKS").is_some();
    let qt_headers = qt_headers();
    let kf6 = kf6_prefix();
    let mut builder = CxxQtBuilder::new_qml_module(module)
        .qrc_resources(QResources::new().resource(QResource::new().prefix("/").files(form)))
        .files(["src/kde.rs"])
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Widgets")
        .qt_module("PrintSupport")
        .qt_module("DBus")
        .qt_module("Network");
    if test_hooks {
        builder = builder.qt_module("Test");
    }
    // SAFETY: the closure only adds include paths, flags and the crate's own source file. It
    // changes none of the settings cxx-qt relies on for the generated code.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            cc.include(qt_headers.join("QtQmlIntegration"));
            cc.flag_if_supported("-Wno-unused-parameter");
            for (include, _) in KF6_MODULES {
                cc.include(kf6.join("include/KF6").join(include));
            }
            cc.file("cpp/kdebridge.cpp");
            if test_hooks {
                cc.define("RK_TEST_HOOKS", None);
                cc.define("QT_GUI_LIB", None);
            }
        })
    };
    // The libraries of the same frameworks; those of the system are found anyway.
    if kf6 != std::path::Path::new("/usr") {
        for directory in ["lib", "lib64"]
            .map(|lib| kf6.join(lib))
            .iter()
            .filter(|directory| directory.is_dir())
        {
            println!("cargo:rustc-link-search=native={}", directory.display());
        }
    }
    for (_, library) in KF6_MODULES {
        println!("cargo:rustc-link-lib={library}");
    }
    // KIOWidgets registers the dialogs of KIO jobs when loaded; no symbol refers to it.
    println!("cargo:rustc-link-arg=-Wl,--no-as-needed,-lKF6KIOWidgets,--as-needed");
    builder.build();
}
