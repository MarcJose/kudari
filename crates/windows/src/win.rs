// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Access to the C++ layer in `cpp/winbridge.cpp`: the global shortcut, the tray icon,
//! notifications and the single instance.

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("winbridge.h");
        /// Whether this is the only Kudari of the user; otherwise it toggles the other one.
        fn rk_win_claim_instance(name: &QString) -> bool;
        /// Registers the shortcut that toggles the window from anywhere, in the portable text
        /// of Qt, in place of the one before. False if Windows refused it.
        fn rk_win_register_hotkey(sequence: &QString) -> bool;
        fn rk_win_setup_tray(toggle_text: &QString, quit_text: &QString);
        fn rk_win_set_tray_visible(visible: bool);
        fn rk_win_notify(title: &QString, text: &QString);
        /// For checks: saves the window as an image after `delay` milliseconds and quits.
        fn rk_win_screenshot_later(path: &QString, delay: i32);
        /// Writes to the console of the caller, for the commands of the command line: the
        /// program has no console of its own, so that none opens with the window.
        fn rk_win_attach_console();
        /// Gives windows an alpha channel; before the first window exists.
        fn rk_win_prepare_translucency();
        /// The icon of the windows and the `FluentWinUI3` style, unless `QT_QUICK_CONTROLS_STYLE`
        /// names another; before the QML loads.
        fn rk_win_use_style();
        /// Light, dark, or as Windows has it: `light`, `dark` or anything else.
        fn rk_win_set_color_scheme(scheme: &QString);
        /// Tells `win_theme_changed` when Windows changes its colors or its scheme.
        fn rk_win_watch_theme();
        /// Blurs what shines through the window, or stops it.
        fn rk_win_set_backdrop(blurred: bool);
    }

    extern "Rust" {
        fn win_hotkey_pressed();
        fn win_quit_requested();
        fn win_theme_changed();
    }
}

pub use ffi::*;

/// The global shortcut, the tray icon and another start toggle the window.
fn win_hotkey_pressed() {
    kudari_qt::controller::queue(kudari_qt::controller::qobject::Controller::toggle_window);
}

/// The colors of the window and the terminals follow those of Windows.
fn win_theme_changed() {
    kudari_qt::controller::queue(kudari_qt::controller::qobject::Controller::theme_changed);
}

fn win_quit_requested() {
    kudari_qt::controller::queue(|controller| controller.trigger_action("file_quit", false));
}
