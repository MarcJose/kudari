// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

use std::fmt::Write;
use std::path::{Path, PathBuf};

use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlModule};

/// Puts the catalogues of `po/` into the program, as a list of language and text.
fn embed_catalogues() {
    let po = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../po");
    println!("cargo:rerun-if-changed={}", po.display());
    let mut languages: Vec<_> = std::fs::read_dir(&po)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().join("kudari.po").is_file())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    languages.sort();
    let mut code = String::from("/// The catalogues of `po/`.\npub const CATALOGUES: &[(&str, &str)] = &[\n");
    for language in languages {
        let path = po.join(&language).join("kudari.po");
        let _ = writeln!(
            code,
            "    ({language:?}, include_str!({:?})),",
            path.display().to_string()
        );
    }
    code.push_str("];\n");
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    if let Err(error) = std::fs::write(Path::new(&out).join("catalogues.rs"), code) {
        println!("cargo:warning=could not write the catalogues: {error}");
    }
}

/// The resource compiler of the newest Windows SDK, where it is installed.
fn resource_compiler() -> Option<PathBuf> {
    let kits = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("x64/rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    // The directories are named by version, which sort as text as long as their parts have
    // the same number of digits, as those of the SDKs do.
    versions.sort();
    versions.pop()
}

/// Gives the program its icon, which Explorer and the taskbar show: a resource that the
/// linker of Visual C++ takes as it is. Only on Windows with that linker.
fn embed_icon() {
    let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/kudari.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let Some(compiler) = resource_compiler() else {
        println!("cargo:warning=no rc.exe in the Windows SDK: the program gets no icon");
        return;
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let script = out.join("kudari.rc");
    let resource = out.join("kudari.res");
    let quoted = icon.display().to_string().replace('\\', "\\\\");
    if let Err(error) = std::fs::write(&script, format!("1 ICON \"{quoted}\"\n")) {
        println!("cargo:warning=could not write {}: {error}", script.display());
        return;
    }
    let status = std::process::Command::new(&compiler)
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg(&script)
        .status();
    match status {
        Ok(status) if status.success() => println!("cargo:rustc-link-arg-bins={}", resource.display()),
        _ => println!("cargo:warning=rc.exe failed: the program gets no icon"),
    }
}

fn main() {
    embed_catalogues();
    embed_icon();
    let module = QmlModule::new("io.github.marcjose.kudari.windows")
        .qml_file("qml/KeySequenceButton.qml")
        .qml_file("qml/Main.qml")
        .qml_file("qml/SettingsWindow.qml")
        .qml_file("qml/ShortcutsPage.qml")
        .qml_file("qml/TabBar.qml");
    println!("cargo:rerun-if-changed=cpp/winbridge.cpp");
    println!("cargo:rerun-if-changed=cpp/winbridge.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=user32");
        println!("cargo:rustc-link-lib=dwmapi");
    }
    // The parts of the settings pages of kudari-qt, in the style of Windows: they import the
    // directory qrc:/qml/form, which each front end fills with its own.
    println!("cargo:rerun-if-changed=qml/form");
    let form = std::fs::read_dir("qml/form")
        .map(|entries| {
            let mut files: Vec<_> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
            files.sort();
            files
        })
        .unwrap_or_default();
    // The icon in its sizes, at qrc:/icons/kudari-SIZE.png.
    let icons: Vec<_> = (std::fs::read_dir("icons").map(|entries| {
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
            .collect();
        files.sort();
        files
    }))
    .unwrap_or_default();
    let builder = CxxQtBuilder::new_qml_module(module)
        .qrc_resources(QResources::new().resource(QResource::new().prefix("/").files(form)))
        .qrc_resources(QResources::new().resource(QResource::new().prefix("/").files(icons)))
        .file("src/win.rs")
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Widgets")
        .qt_module("Network");
    // SAFETY: the closure only adds an include path and the crate's own source file. It changes none
    // of the settings cxx-qt relies on for the generated code.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            cc.file("cpp/winbridge.cpp");
        })
    };
    builder.build();
}
