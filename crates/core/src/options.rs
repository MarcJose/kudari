// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The options of the terminals, which the front end derives from the settings.

use std::sync::Arc;

use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use crate::colors::parse_color;
use crate::input::Modifiers;
use crate::keytab;
use crate::settings::Settings;

/// Terminal settings, applied to new and running terminals alike.
#[derive(Clone, Debug)]
pub struct Options {
    pub history_lines: usize,
    pub cursor: CursorShape,
    pub blinking: bool,
    pub kitty_keyboard: bool,
    pub word_separators: String,
    pub command: String,
    pub shell: String,
    pub shell_integration: bool,
    pub directory: String,
    /// One `NAME=value` per line, for new terminals.
    pub environment: String,
    pub flow_control: bool,
    pub remote_title_format: String,
    pub margin: i32,
    pub center: bool,
    /// Column of the vertical line, or zero.
    pub vertical_line: i32,
    pub line_numbers: bool,
    pub badge: Option<Badge>,
    pub cursor_color: Option<Rgb>,
    pub cursor_text_color: Option<Rgb>,
    pub anti_alias: bool,
    pub hinting: bool,
    pub triple_click_forward: bool,
    pub trim_leading: bool,
    pub trim_trailing: bool,
    pub clear_on_copy: bool,
    pub copy_html: bool,
    pub open_links_by_click: bool,
    pub mouse_tracking: bool,
    pub editor_command: String,
    pub scroll_full_page: bool,
    pub escaped_links: bool,
    pub key_rules: Arc<Vec<keytab::Rule>>,
    /// The modifiers that show the link numbers; none switches them off.
    pub url_hints_modifiers: Modifiers,
    pub reverse_url_hints: bool,
    pub emoji_font: String,
    pub own_line_characters: bool,
    pub own_braille_characters: bool,
    pub invert_selection: bool,
    pub link_schemes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Badge {
    pub format: String,
    pub color: String,
    pub family: String,
    pub size: i32,
    pub text_only: bool,
    pub transparency: i32,
}

/// Maps the `CursorShape` setting; unknown values mean the block cursor.
#[must_use]
pub fn cursor_shape(name: &str) -> CursorShape {
    match name {
        "underline" => CursorShape::Underline,
        "beam" => CursorShape::Beam,
        _ => CursorShape::Block,
    }
}

impl Options {
    /// The options the settings ask for. Reads the key binding file they name.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            history_lines: settings.history_lines as usize,
            cursor: cursor_shape(&settings.cursor_shape),
            blinking: settings.blinking_cursor,
            kitty_keyboard: settings.kitty_keyboard,
            word_separators: settings.word_separators.clone(),
            command: settings.command.clone(),
            shell: settings.shell.clone(),
            shell_integration: settings.shell_integration,
            directory: settings.directory.clone(),
            environment: settings.environment.clone(),
            flow_control: settings.flow_control,
            remote_title_format: settings.remote_title_format.clone(),
            margin: settings.margin,
            center: settings.center_content,
            vertical_line: settings.vertical_line,
            line_numbers: settings.line_numbers,
            badge: settings.badge_enabled.then(|| Badge {
                format: settings.badge_text.clone(),
                color: settings.badge_color.clone(),
                family: settings.badge_font_family.clone(),
                size: settings.badge_font_size,
                text_only: settings.badge_text_only,
                transparency: settings.badge_transparency,
            }),
            cursor_color: parse_color(&settings.cursor_color),
            cursor_text_color: parse_color(&settings.cursor_text_color),
            anti_alias: settings.anti_alias_fonts,
            hinting: settings.font_hinting,
            triple_click_forward: settings.triple_click == "forward",
            trim_leading: settings.trim_leading_spaces,
            trim_trailing: settings.trim_trailing_spaces,
            clear_on_copy: settings.clear_selection_on_copy,
            copy_html: settings.copy_as_html,
            open_links_by_click: settings.open_links_by_click,
            mouse_tracking: settings.mouse_tracking,
            editor_command: settings.editor_command.clone(),
            scroll_full_page: settings.scroll_full_page,
            escaped_links: settings.escaped_links,
            key_rules: Arc::new(keytab::load(&settings.key_bindings)),
            url_hints_modifiers: Modifiers::from_names(&settings.url_hints_modifier),
            reverse_url_hints: settings.reverse_url_hints,
            emoji_font: settings.emoji_font.clone(),
            own_line_characters: !settings.font_line_characters,
            own_braille_characters: !settings.font_braille_characters,
            invert_selection: settings.invert_selection,
            link_schemes: settings
                .link_schemes
                .split(';')
                .map(str::trim)
                .filter(|scheme| !scheme.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

/// What applies until the settings arrive: as safe as the settings themselves by default.
impl Default for Options {
    fn default() -> Self {
        Self {
            history_lines: 0,
            cursor: CursorShape::default(),
            blinking: false,
            kitty_keyboard: false,
            word_separators: String::new(),
            command: String::new(),
            shell: String::new(),
            shell_integration: true,
            directory: String::new(),
            environment: String::new(),
            flow_control: true,
            remote_title_format: String::new(),
            margin: 0,
            center: false,
            vertical_line: 0,
            line_numbers: false,
            badge: None,
            cursor_color: None,
            cursor_text_color: None,
            anti_alias: false,
            hinting: false,
            triple_click_forward: false,
            trim_leading: false,
            trim_trailing: false,
            clear_on_copy: false,
            copy_html: false,
            open_links_by_click: false,
            mouse_tracking: true,
            editor_command: String::new(),
            scroll_full_page: false,
            escaped_links: true,
            key_rules: Arc::default(),
            url_hints_modifiers: Modifiers::NONE,
            reverse_url_hints: false,
            emoji_font: String::new(),
            own_line_characters: false,
            own_braille_characters: false,
            invert_selection: false,
            link_schemes: ["http://", "https://", "mailto:"].map(String::from).to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Store;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Table(RefCell<HashMap<String, String>>);

    impl Store for Table {
        fn read(&self, _: &str, key: &str, default: &str) -> String {
            self.0.borrow().get(key).cloned().unwrap_or_else(|| default.to_string())
        }

        fn write(&self, _: &str, key: &str, value: &str) {
            self.0.borrow_mut().insert(key.to_string(), value.to_string());
        }
    }

    #[test]
    fn options_follow_the_settings() {
        let table = Table::default();
        for (key, value) in [
            ("CursorShape", "beam"),
            ("CursorColor", "#102030"),
            ("TripleClick", "forward"),
            ("LinkSchemes", "https:// ; ssh:// ;"),
            ("UrlHintsModifier", "ctrl+alt"),
            ("BadgeEnabled", "true"),
            ("BadgeText", "%n"),
            ("FontLineCharacters", "true"),
            ("FontBrailleCharacters", "false"),
        ] {
            table.write("", key, value);
        }
        let options = Options::from_settings(&Settings::load(&table));
        assert_eq!(options.cursor, CursorShape::Beam);
        assert_eq!(
            options.cursor_color,
            Some(Rgb {
                r: 0x10,
                g: 0x20,
                b: 0x30
            })
        );
        assert_eq!(options.cursor_text_color, None);
        assert!(options.triple_click_forward);
        assert_eq!(options.link_schemes, ["https://", "ssh://"]);
        assert_eq!(options.url_hints_modifiers, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(options.badge.map(|badge| badge.format).as_deref(), Some("%n"));
        assert!(!options.own_line_characters && options.own_braille_characters);
        assert_eq!(cursor_shape("underline"), CursorShape::Underline);
        assert_eq!(cursor_shape("unknown"), CursorShape::Block);
    }
}
