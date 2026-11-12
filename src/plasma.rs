// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The services of KDE Plasma for the core: the clipboard of Qt, `KNotification`, `KIO`,
//! the color schemes of the global themes, the screens and the strut manager of Plasma,
//! the window effects of `KWin` and `KGlobalAccel`.

use kudari_qt::frontend::{Feature, Frontend, Skin};
use kudari_qt::qt;

use crate::app::{Confirm, Question};
use crate::config::KConfig;
use crate::kde::{self, qs, tr};
use crate::migrate::{self, Paths};
use crate::services::{Clipboard, Desktop, GlobalShortcut, Notifications, Screens, Selection, Themes, Window};
use crate::settings::{self, Store};
use crate::theme::Palette;

/// The services of KDE Plasma.
pub struct Plasma;

impl Clipboard for Plasma {
    fn copy(&self, text: &str, target: Selection) {
        qt::rk_clipboard_set(&qs(text), target == Selection::Primary);
    }

    fn copy_rich(&self, text: &str, html: &str) {
        qt::rk_clipboard_set_rich(&qs(text), &qs(html));
    }

    fn paste(&self, source: Selection) -> String {
        qt::rk_clipboard_get(source == Selection::Primary).to_string()
    }
}

impl Notifications for Plasma {
    fn notify(&self, event: &str, text: &str) {
        kde::rk_notify(&qs(event), &qs(text));
    }
}

impl Desktop for Plasma {
    fn open_url(&self, url: &str) {
        kde::rk_open_url(&qs(url));
    }

    fn web_search(&self, text: &str) {
        kde::rk_web_search(&qs(text));
    }
}

impl Themes for Plasma {
    fn palette(&self, theme: Option<&str>) -> Option<Palette> {
        crate::theme::parse_palette(&kde::rk_theme_colors(&qs(theme.unwrap_or_default())).to_string())
    }

    fn installed(&self) -> Vec<(String, String)> {
        let themes: Vec<serde_json::Value> =
            serde_json::from_str(&kde::rk_global_themes_json().to_string()).unwrap_or_default();
        themes
            .iter()
            .filter_map(|theme| {
                Some((
                    theme.get("id")?.as_str()?.to_string(),
                    theme.get("name")?.as_str()?.to_string(),
                ))
            })
            .collect()
    }
}

impl Screens for Plasma {
    fn outputs(&self) -> Vec<String> {
        let names = kde::rk_output_names().to_string();
        names
            .split(',')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn pointer_output(&self) -> Option<String> {
        crate::dbus::pointer_output()
    }

    fn active_output(&self) -> String {
        kde::rk_active_output_name().to_string()
    }

    /// Plasma reports the area left free by panels over D-Bus.
    fn work_area(&self, output: &str) -> Option<(i32, i32)> {
        parse_size(&kde::rk_available_screen_rect(&qs(output)).to_string())
    }
}

impl Window for Plasma {
    fn slide(&self) {
        kde::rk_slide_window();
    }

    fn set_blur(&self, enabled: bool) {
        kde::rk_set_blur(enabled);
    }

    /// KDE has its color schemes and themes for that.
    fn set_color_scheme(&self, _scheme: &str) {}
}

impl GlobalShortcut for Plasma {
    fn toggle(&self) -> String {
        kde::rk_toggle_shortcut().to_string()
    }

    /// KDE asks itself before it takes a shortcut from another program.
    fn set_toggle(&self, sequence: &str) -> bool {
        kde::rk_set_toggle_shortcut(&qs(sequence));
        true
    }

    /// The shortcut of Yakuake.
    fn foreign_toggle(&self) -> String {
        kde::rk_foreign_toggle_shortcut().to_string()
    }
}

impl Frontend for Plasma {
    fn store(&self) -> &dyn Store {
        &KConfig
    }

    fn set_action_state(&self, name: &str, checked: bool, enabled: bool) {
        kde::rk_set_action_state(&qs(name), checked, enabled);
    }

    fn action_text(&self, name: &str) -> String {
        kde::rk_action_text(&qs(name)).to_string()
    }

    fn shortcuts_json(&self) -> String {
        kde::rk_shortcuts_json().to_string()
    }

    fn reload_shortcuts(&self) {
        kde::rk_reload_shortcuts();
    }

    fn set_menu_state(&self, json: &str) {
        kde::rk_set_menu_state(&qs(json));
    }

    fn set_tray_visible(&self, visible: bool) {
        kde::rk_set_tray_visible(visible);
    }

    fn highlight_color(&self) -> String {
        kde::rk_highlight_color().to_string()
    }

    fn skins(&self) -> Vec<Skin> {
        let system = Skin {
            id: settings::SYSTEM_SKIN.to_string(),
            name: tr("@item:inlistbox", "System theme"),
            description: tr("@info", "Follows the active Plasma theme."),
        };
        let themes = crate::theme::skins(self).into_iter().map(|(id, name)| Skin {
            id,
            name,
            description: tr("@info", "Plasma theme"),
        });
        std::iter::once(system).chain(themes).collect()
    }

    fn popup_main_menu(&self, x: i32, y: i32) -> Option<String> {
        kde::rk_popup_main_menu(x, y);
        None
    }

    fn popup_session_menu(&self, x: i32, y: i32) -> Option<String> {
        kde::rk_popup_session_menu(x, y);
        None
    }

    fn popup_tab_menu(&self, x: i32, y: i32, state: &str) -> Option<String> {
        kde::rk_popup_tab_menu(x, y, &qs(state));
        None
    }

    fn popup_terminal_menu(&self, x: i32, y: i32, can_copy: bool, link: &str, auto_saving: bool) -> Option<String> {
        kde::rk_popup_terminal_menu(x, y, can_copy, &qs(link), auto_saving);
        None
    }

    fn popup_drop_menu(&self, x: i32, y: i32, terminal: i32, paths: &str, directory: &str) {
        kde::rk_popup_drop_menu(x, y, terminal, &qs(paths), &qs(directory));
    }

    fn ask(&self, question: &Question) -> bool {
        let kind = match question.confirm {
            Confirm::Quit => kde::Confirm::Quit,
            Confirm::Close => kde::Confirm::Close,
            Confirm::Continue => kde::Confirm::Continue,
        };
        kde::rk_ask_question(
            question.id as i32,
            &qs(&question.text),
            &qs(&question.title),
            kind as i32,
        );
        true
    }

    fn ask_agent(&self, agent: String) {
        crate::agent::ask_user(agent);
    }

    fn close_agent_question(&self) {
        kde::rk_close_agent_question();
    }

    /// KDE changes shortcuts in its own dialog.
    fn set_shortcut(&self, _name: &str, _sequences: &[String]) {}

    /// Everything but a choice of shells and of light or dark, which are Windows' business.
    fn supports(&self, feature: Feature) -> bool {
        !matches!(feature, Feature::Shells | Feature::ColorScheme)
    }

    fn can_import(&self) -> bool {
        Paths::from_env().and_then(|paths| migrate::read(&paths)).is_some()
    }

    /// Takes over the settings, key tables, color schemes and shortcuts of Yakuake and Konsole.
    fn import(&self) -> bool {
        let Some(paths) = Paths::from_env() else {
            return false;
        };
        let Some(migration) = migrate::read(&paths) else {
            return false;
        };
        if let Err(error) = migration.copy_files(&paths) {
            eprintln!("kudari: could not copy imported files: {error}");
        }
        for (key, value) in &migration.settings {
            settings::store(&KConfig, key, value);
        }
        for (action, sequences) in &migration.shortcuts {
            kde::rk_config_write(&qs("Shortcuts"), &qs(action), &qs(sequences));
        }
        true
    }
}

/// The size from the `x,y,width,height` of `rk_available_screen_rect`.
fn parse_size(text: &str) -> Option<(i32, i32)> {
    let numbers: Vec<i32> = text.split(',').filter_map(|value| value.parse().ok()).collect();
    match numbers[..] {
        [_, _, width, height] => Some((width, height)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_read_in_order_and_checked() {
        let text = "#101010,#202020,#303030,#404040,#505050,#606060,#707070,#808080,#909090,\
                    #a0a0a0,#b0b0b0,#c0c0c0,#d0d0d0,#e0e0e0,#f0f0f0";
        let palette = crate::theme::parse_palette(text).unwrap();
        assert_eq!(palette.header_background, "#101010");
        assert_eq!(palette.highlight, "#505050");
        assert_eq!(palette.inactive, "#f0f0f0");
        assert_eq!(crate::theme::parse_palette("#101010,#202020"), None);
        assert_eq!(crate::theme::parse_palette(""), None);
    }

    #[test]
    fn the_free_area_is_read_from_its_rectangle() {
        assert_eq!(parse_size("0,30,1920,1050"), Some((1920, 1050)));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("1,2,3"), None);
    }

    #[test]
    fn random_answers_do_no_harm() {
        for input in crate::fuzz::inputs(&["#101010,#202020,#303030,#404040", "0,30,1920,1050"]) {
            let _ = crate::theme::parse_palette(&input);
            let _ = parse_size(&input);
            let _ = crate::settings::is_themed(&input);
        }
    }
}
