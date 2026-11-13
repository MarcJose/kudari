// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What a front end provides of its platform: the clipboard, notifications, opening links,
//! the colors of the desktop theme, the screens and the window on them, and the global
//! shortcut. The core asks through these traits, so every platform answers in its own way:
//! KDE Plasma through Qt, KDE Frameworks and D-Bus, Windows through its own interfaces.

use crate::theme::Palette;

/// Where copied text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// What Ctrl+C and Ctrl+V use.
    Clipboard,
    /// The selection that a middle click pastes. Platforms without one ignore it.
    Primary,
}

pub trait Clipboard {
    fn copy(&self, text: &str, target: Selection);
    /// Copies to the clipboard with formatting for programs that take it.
    fn copy_rich(&self, text: &str, html: &str);
    fn paste(&self, source: Selection) -> String;
}

pub trait Notifications {
    /// Shows a notification. `event` names its kind, such as `bell`, for the settings of
    /// the platform.
    fn notify(&self, event: &str, text: &str);
}

pub trait Desktop {
    /// Opens a web address, or a file or folder with the program the user chose for it.
    fn open_url(&self, url: &str);
    /// Searches the web for the text with the search engine the user chose.
    fn web_search(&self, text: &str);
}

pub trait Themes {
    /// The colors of an installed theme, or of the active one for `None`.
    fn palette(&self, theme: Option<&str>) -> Option<Palette>;
    /// The installed themes as id and name.
    fn installed(&self) -> Vec<(String, String)>;
}

pub trait Screens {
    /// The names of the connected screens, in the order of the `Screen` setting.
    fn outputs(&self) -> Vec<String>;
    /// The screen under the mouse pointer, if the platform tells.
    fn pointer_output(&self) -> Option<String>;
    /// The screen with the active window.
    fn active_output(&self) -> String;
    /// The size of the area that panels and task bars leave free on a screen.
    fn work_area(&self, output: &str) -> Option<(i32, i32)>;
}

pub trait Window {
    /// Lets the window slide in from the edge of the screen when it shows next.
    fn slide(&self);
    /// Blurs what shines through the translucent window.
    fn set_blur(&self, enabled: bool);
    /// Light or dark as the `ColorScheme` setting says, where the front end chooses that.
    fn set_color_scheme(&self, scheme: &str);
}

pub trait GlobalShortcut {
    /// The key sequence that opens and closes the window from anywhere.
    fn toggle(&self) -> String;
    /// Makes `sequence` the shortcut. False if the system refused it, which then keeps the
    /// one before.
    fn set_toggle(&self, sequence: &str) -> bool;
    /// The sequence of another drop-down terminal, which the first start offers to take over.
    fn foreign_toggle(&self) -> String;
}

/// Everything a front end provides.
pub trait Services: Clipboard + Notifications + Desktop + Themes + Screens + Window + GlobalShortcut {}

impl<T: Clipboard + Notifications + Desktop + Themes + Screens + Window + GlobalShortcut> Services for T {}

/// The screen for the window and its free area.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkArea {
    pub output: String,
    pub width: i32,
    pub height: i32,
}

/// `MainWindow::getScreen` and `getDesktopGeometry`: the screen of the `Screen` setting,
/// counted from 1, or with 0 the one under the pointer, else the one with the active window.
#[must_use]
pub fn work_area(screens: &dyn Screens, screen: i32) -> WorkArea {
    let names = screens.outputs();
    let configured = usize::try_from(screen - 1).ok().and_then(|index| names.get(index));
    let output = configured
        .cloned()
        .or_else(|| screens.pointer_output().filter(|output| names.contains(output)))
        .unwrap_or_else(|| screens.active_output());
    let (width, height) = screens
        .work_area(&output)
        .filter(|&(width, height)| width > 0 && height > 0)
        .unwrap_or_default();
    WorkArea { output, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Desk {
        outputs: Vec<String>,
        pointer: Option<String>,
    }

    impl Screens for Desk {
        fn outputs(&self) -> Vec<String> {
            self.outputs.clone()
        }

        fn pointer_output(&self) -> Option<String> {
            self.pointer.clone()
        }

        fn active_output(&self) -> String {
            "active".into()
        }

        fn work_area(&self, output: &str) -> Option<(i32, i32)> {
            match output {
                "DP-1" => Some((1920, 1050)),
                "HDMI-1" => Some((0, 0)),
                _ => None,
            }
        }
    }

    fn desk(pointer: Option<&str>) -> Desk {
        Desk {
            outputs: vec!["DP-1".into(), "HDMI-1".into()],
            pointer: pointer.map(str::to_string),
        }
    }

    #[test]
    fn a_configured_screen_wins() {
        let area = work_area(&desk(Some("HDMI-1")), 1);
        assert_eq!(
            area,
            WorkArea {
                output: "DP-1".into(),
                width: 1920,
                height: 1050
            }
        );
    }

    #[test]
    fn without_a_setting_the_window_follows_the_pointer() {
        assert_eq!(work_area(&desk(Some("DP-1")), 0).output, "DP-1");
        assert_eq!(work_area(&desk(Some("DP-1")), 3).output, "DP-1");
    }

    #[test]
    fn an_unknown_pointer_screen_falls_back_to_the_active_one() {
        assert_eq!(work_area(&desk(Some("gone")), 0).output, "active");
        assert_eq!(work_area(&desk(None), 0).output, "active");
    }

    #[test]
    fn an_empty_or_unknown_area_has_no_size() {
        assert_eq!(
            work_area(&desk(Some("HDMI-1")), 0),
            WorkArea {
                output: "HDMI-1".into(),
                width: 0,
                height: 0
            }
        );
        assert_eq!(work_area(&desk(None), 0).width, 0);
    }
}
