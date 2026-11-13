// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Import of an existing Yakuake setup, which is only ever read, never changed.
//!
//! Yakuake's terminals look the way Konsole's default profile says, so the import reads
//! that profile as well. The security settings have no counterpart and stay untouched.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ini::parse_ini;

/// Settings that mean the same in `yakuakerc` and `kudarirc`.
const SHARED_SETTINGS: [(&str, &str); 18] = [
    ("Window", "Screen"),
    ("Window", "Width"),
    ("Window", "Height"),
    ("Window", "Position"),
    ("Window", "KeepOpen"),
    ("Window", "KeepOpenAfterLastSessionCloses"),
    ("Window", "ToggleToFocus"),
    ("Window", "ShowTabBar"),
    ("Window", "DynamicTabTitles"),
    ("Window", "ShowSystrayIcon"),
    ("Behavior", "FocusFollowsMouse"),
    ("Behavior", "OpenAfterStart"),
    ("Behavior", "RememberFullscreen"),
    ("Appearance", "Translucency"),
    ("Appearance", "BackgroundColorOpacity"),
    ("Appearance", "TerminalHighlightOnManualActivation"),
    ("Appearance", "HideSkinBorders"),
    ("Dialogs", "ConfirmQuit"),
];

type Ini = HashMap<String, HashMap<String, String>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    config: PathBuf,
    data: PathBuf,
}

impl Paths {
    pub fn new(config: PathBuf, data: PathBuf) -> Self {
        Self { config, data }
    }

    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let directory = |variable: &str, fallback: &str| {
            std::env::var_os(variable)
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(fallback)))
        };
        Some(Self::new(
            directory("XDG_CONFIG_HOME", ".config")?,
            directory("XDG_DATA_HOME", ".local/share")?,
        ))
    }
}

/// What can be taken over from Yakuake and Konsole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Migration {
    /// Key and value pairs for `settings::store`.
    pub settings: Vec<(String, String)>,
    /// Action name and key sequences, in the format of the `[Shortcuts]` group.
    pub shortcuts: Vec<(String, String)>,
    /// A key binding file that has to be copied.
    pub key_table: Option<PathBuf>,
}

fn read_ini(path: &Path) -> Option<Ini> {
    fs::read_to_string(path).ok().map(|text| parse_ini(&text))
}

/// A file or directory name from a configuration file must stay inside its directory.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// `KConfig` stores colors as `r,g,b`, QML expects `#rrggbb`.
fn color_name(value: &str) -> Option<String> {
    if value.starts_with('#') {
        return Some(value.to_string());
    }
    let channels: Vec<u8> = value
        .split(',')
        .filter_map(|channel| channel.trim().parse().ok())
        .collect();
    match channels[..] {
        [r, g, b, ..] => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        _ => None,
    }
}

/// The family and size of a serialized `QFont` such as `Hack,11,-1,5,400,0,0,0,0,0`.
fn font_parts(value: &str) -> Option<(String, i32)> {
    let mut parts = value.split(',');
    let family = parts.next().filter(|family| !family.is_empty())?;
    let size = parts.next()?.parse::<f64>().ok().filter(|size| *size > 0.0)?;
    Some((family.to_string(), size.round() as i32))
}

/// History length from Konsole's `HistoryMode`: none, fixed or unlimited.
fn history_lines(mode: Option<&String>, size: Option<&String>) -> Option<String> {
    match mode.map(String::as_str) {
        Some("0") => Some("0".to_string()),
        Some("2") => Some("1000000".to_string()),
        _ => size.cloned(),
    }
}

fn cursor_shape(value: &str) -> Option<&'static str> {
    match value {
        "0" => Some("block"),
        "1" => Some("beam"),
        "2" => Some("underline"),
        _ => None,
    }
}

/// Konsole counts positions as left, right and hidden.
fn scroll_bar_position(value: &str) -> Option<&'static str> {
    match value {
        "0" => Some("left"),
        "1" => Some("right"),
        "2" => Some("hidden"),
        _ => None,
    }
}

fn default_profile(paths: &Paths, yakuake: Option<&Ini>) -> Option<Ini> {
    let konsole = read_ini(&paths.config.join("konsolerc"));
    let name = [yakuake, konsole.as_ref()]
        .into_iter()
        .flatten()
        .find_map(|ini| ini.get("Desktop Entry")?.get("DefaultProfile"))
        .filter(|name| is_plain_name(name))?;
    read_ini(&paths.data.join("konsole").join(name))
}

/// Konsole options with the same meaning here: group, name there, name here.
const COPIED_PROFILE_OPTIONS: [(&str, &str, &str); 26] = [
    ("General", "RemoteTabTitleFormat", "RemoteTitleFormat"),
    ("General", "TerminalCenter", "CenterContent"),
    ("General", "DimWhenInactive", "DimInactive"),
    ("Appearance", "DimmValue", "DimValue"),
    ("Appearance", "AntiAliasFonts", "AntiAliasFonts"),
    ("Appearance", "FontHinting", "FontHinting"),
    ("Appearance", "BorderWhenActive", "BorderWhenActive"),
    ("Scrolling", "ScrollFullPage", "ScrollFullPage"),
    ("Terminal Features", "FlowControlEnabled", "FlowControl"),
    (
        "Interaction Options",
        "TrimLeadingSpacesInSelectedText",
        "TrimLeadingSpaces",
    ),
    (
        "Interaction Options",
        "TrimTrailingSpacesInSelectedText",
        "TrimTrailingSpaces",
    ),
    ("Interaction Options", "ClearSelectionOnCopy", "ClearSelectionOnCopy"),
    ("Interaction Options", "CopyTextAsHTML", "CopyAsHtml"),
    (
        "Interaction Options",
        "OpenLinksByDirectClickEnabled",
        "OpenLinksByClick",
    ),
    ("Interaction Options", "UnderlineLinksEnabled", "UnderlineLinks"),
    ("Interaction Options", "UnderlineFilesEnabled", "UnderlineFiles"),
    ("Interaction Options", "AllowMouseTracking", "MouseTracking"),
    ("Interaction Options", "CtrlRequiredForDrag", "DragNeedsCtrl"),
    ("Interaction Options", "DropUrlsAsText", "DropUrlsAsText"),
    ("Interaction Options", "ColorFilterEnabled", "ColorFilter"),
    ("Scrolling", "HighlightScrolledLines", "HighlightScrolledLines"),
    ("Scrolling", "MarkerSize", "MarkerSize"),
    ("Scrolling", "SearchLineOpacity", "SearchLineOpacity"),
    ("Appearance", "UseFontLineChararacters", "FontLineCharacters"),
    ("Appearance", "UseFontBrailleChararacters", "FontBrailleCharacters"),
    ("General", "StartInCurrentSessionDir", "InheritDirectory"),
];

fn read_profile(profile: &Ini, paths: &Paths, migration: &mut Migration) {
    let value = |group: &str, key: &str| profile.get(group).and_then(|group| group.get(key));
    let mut set = |key: &str, value: String| migration.settings.push((key.to_string(), value));

    if let Some((family, size)) = value("Appearance", "Font").and_then(|font| font_parts(font)) {
        set("FontFamily", family);
        set("FontSize", size.to_string());
    }
    if let Some(lines) = history_lines(value("Scrolling", "HistoryMode"), value("Scrolling", "HistorySize")) {
        set("HistoryLines", lines);
    }
    if let Some(shape) = value("Cursor Options", "CursorShape").and_then(|shape| cursor_shape(shape)) {
        set("CursorShape", shape.to_string());
    }
    if let Some(blinking) = value("Cursor Options", "BlinkingCursorEnabled") {
        set("BlinkingCursor", blinking.clone());
    }
    if let Some(position) = value("Scrolling", "ScrollBarPosition").and_then(|position| scroll_bar_position(position)) {
        set("ScrollBarPosition", position.to_string());
    }
    if let Some(format) = value("General", "LocalTabTitleFormat") {
        set("TitleFormat", format.clone());
    }
    if let Some(command) = value("General", "Command") {
        set("Command", command.clone());
    }
    if let Some(seconds) = value("General", "SilenceSeconds") {
        set("SilenceSeconds", seconds.clone());
    }
    if let Some(spacing) = value("Appearance", "LineSpacing") {
        set("LineSpacing", spacing.clone());
    }
    if let Some(bold) = value("Appearance", "BoldIntense") {
        set("BoldIntense", bold.clone());
    }
    for (group, source, target) in COPIED_PROFILE_OPTIONS {
        if let Some(option) = value(group, source) {
            set(target, option.clone());
        }
    }
    if let Some(margin) = value("General", "TerminalMargin").and_then(|margin| margin.parse::<i32>().ok()) {
        // Konsole counts its built-in margin of one pixel, which Kudari always has.
        set("Margin", (margin - 1).max(0).to_string());
    }
    if value("Terminal Features", "VerticalLine").is_some_and(|enabled| enabled == "true") {
        let column = value("Terminal Features", "VerticalLineAtChar").map_or("80", String::as_str);
        set("VerticalLine", column.to_string());
    }
    if value("Terminal Features", "LineNumbers").is_some_and(|mode| mode == "2") {
        set("LineNumbers", "true".to_string());
    }
    if value("Interaction Options", "TripleClickMode").is_some_and(|mode| mode == "1") {
        set("TripleClick", "forward".to_string());
    }
    if let Some(name) = value("Keyboard", "KeyBindings").filter(|name| is_plain_name(name)) {
        let file = paths.data.join("konsole").join(format!("{name}.keytab"));
        if file.is_file() {
            migration.key_table = Some(file);
            set("KeyBindings", name.clone());
        }
    }
    if let Some(mode) = value("Interaction Options", "RightClickPasteMode") {
        let mode = match mode.as_str() {
            "1" => "selection",
            "2" => "clipboard",
            _ => "none",
        };
        set("RightClickPaste", mode.to_string());
    }
    if let Some(environment) = value("General", "Environment") {
        // A comma-separated list, of which `TERM` and `COLORTERM` are Kudari's own business.
        let custom: Vec<&str> = environment
            .split(',')
            .filter(|entry| !entry.starts_with("TERM=") && !entry.starts_with("COLORTERM="))
            .collect();
        set("Environment", custom.join("\n"));
    }
    if let Some(interval) =
        value("Terminal Features", "AutoSaveInterval").and_then(|interval| interval.parse::<i32>().ok())
    {
        set("AutoSaveSeconds", (interval / 1000).max(1).to_string());
    }

    let Some(scheme) = value("Appearance", "ColorScheme").filter(|scheme| is_plain_name(scheme)) else {
        return;
    };
    // Colors come from the Plasma theme, but transparency is part of Konsole's color scheme.
    let Some(ini) = read_ini(&paths.data.join("konsole").join(format!("{scheme}.colorscheme"))) else {
        return;
    };
    let general = ini.get("General");
    let opacity = general
        .and_then(|general| general.get("Opacity"))
        .and_then(|opacity| opacity.parse::<f64>().ok());
    if let Some(opacity) = opacity {
        set("Opacity", ((opacity * 100.0).round() as i32).clamp(10, 100).to_string());
    }
    if let Some(blur) = general.and_then(|general| general.get("Blur")) {
        set("Blur", blur.clone());
    }
}

/// Reads what can be imported, or `None` if neither Yakuake nor Konsole left anything.
pub fn read(paths: &Paths) -> Option<Migration> {
    let yakuake = read_ini(&paths.config.join("yakuakerc"));
    let profile = default_profile(paths, yakuake.as_ref());
    if yakuake.is_none() && profile.is_none() {
        return None;
    }

    let mut migration = Migration::default();
    if let Some(ini) = &yakuake {
        for (group, key) in SHARED_SETTINGS {
            if let Some(value) = ini.get(group).and_then(|group| group.get(key)) {
                migration.settings.push((key.to_string(), value.clone()));
            }
        }
        if let Some(color) = ini
            .get("Appearance")
            .and_then(|group| group.get("BackgroundColor"))
            .and_then(|color| color_name(color))
        {
            migration.settings.push(("BackgroundColor".to_string(), color));
        }

        // The global shortcut lives in KGlobalAccel and is chosen in the first-run dialog.
        if let Some(shortcuts) = ini.get("Shortcuts") {
            migration.shortcuts = shortcuts
                .iter()
                .filter(|(name, value)| is_action_name(name) && *name != "toggle-window-state" && !value.is_empty())
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            migration.shortcuts.sort();
        }
    }
    if let Some(profile) = &profile {
        read_profile(profile, paths, &mut migration);
    }
    Some(migration)
}

impl Migration {
    /// Copies the key bindings into Kudari's data directory.
    pub fn copy_files(&self, paths: &Paths) -> io::Result<()> {
        if let (Some(source), Some(file_name)) = (
            &self.key_table,
            self.key_table.as_ref().and_then(|source| source.file_name()),
        ) {
            let directory = paths.data.join("kudari/keytabs");
            fs::create_dir_all(&directory)?;
            fs::copy(source, directory.join(file_name))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A throwaway home with `config` and `data` directories.
    struct Home(PathBuf);

    impl Home {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("kudari-test-{}-{id}", std::process::id()));
            fs::create_dir_all(root.join("config")).unwrap();
            fs::create_dir_all(root.join("data/konsole")).unwrap();
            Self(root)
        }

        fn paths(&self) -> Paths {
            Paths::new(self.0.join("config"), self.0.join("data"))
        }

        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read_with_defaults(home: &Home) -> Option<Migration> {
        read(&home.paths())
    }

    fn setting<'a>(migration: &'a Migration, key: &str) -> Option<&'a str> {
        migration
            .settings
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn nothing_to_import_without_yakuake_and_konsole() {
        assert_eq!(read_with_defaults(&Home::new()), None);
    }

    #[test]
    fn window_settings_and_colors_are_taken_over() {
        let home = Home::new();
        home.write(
            "config/yakuakerc",
            "[Window]\nWidth=70\nKeepOpen=false\n[Appearance]\nBackgroundColor=255,0,16\n",
        );
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(setting(&migration, "Width"), Some("70"));
        assert_eq!(setting(&migration, "KeepOpen"), Some("false"));
        assert_eq!(setting(&migration, "BackgroundColor"), Some("#ff0010"));
    }

    #[test]
    fn security_and_first_run_state_are_not_taken_over() {
        let home = Home::new();
        home.write(
            "config/yakuakerc",
            "[Security]\nAllowRemoteCommands=true\n[Dialogs]\nFirstRun=false\n",
        );
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(setting(&migration, "AllowRemoteCommands"), None);
        assert_eq!(setting(&migration, "FirstRun"), None);
    }

    #[test]
    fn shortcuts_keep_their_format_but_not_the_global_one() {
        let home = Home::new();
        home.write(
            "config/yakuakerc",
            "[Shortcuts]\nnew-session=Ctrl+N; Alt+N\ntoggle-window-state=F11\nbad name=x\n",
        );
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(
            migration.shortcuts,
            vec![("new-session".to_string(), "Ctrl+N; Alt+N".to_string())]
        );
    }

    #[test]
    fn the_konsole_profile_provides_the_terminal_look() {
        let home = Home::new();
        home.write("config/konsolerc", "[Desktop Entry]\nDefaultProfile=Mine.profile\n");
        home.write(
            "data/konsole/Mine.profile",
            "[Appearance]\nColorScheme=Own\nFont=Hack,11.5,-1,5,400\n[Scrolling]\nHistoryMode=1\nHistorySize=5000\n[Cursor Options]\nCursorShape=1\n",
        );
        home.write("data/konsole/Own.colorscheme", "[General]\nOpacity=0.85\nBlur=true\n");
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(setting(&migration, "FontFamily"), Some("Hack"));
        assert_eq!(setting(&migration, "FontSize"), Some("12"));
        assert_eq!(setting(&migration, "HistoryLines"), Some("5000"));
        assert_eq!(setting(&migration, "CursorShape"), Some("beam"));
        assert_eq!(setting(&migration, "Opacity"), Some("85"));
        assert_eq!(setting(&migration, "Blur"), Some("true"));
    }

    #[test]
    fn display_and_interaction_options_are_taken_over() {
        let home = Home::new();
        home.write("config/konsolerc", "[Desktop Entry]\nDefaultProfile=Mine.profile\n");
        home.write(
            "data/konsole/Mine.profile",
            "[General]\nEnvironment=TERM=xterm-256color,COLORTERM=truecolor,EDITOR=vim\nTerminalMargin=5\n\
             TerminalCenter=true\n[Terminal Features]\nVerticalLine=true\nVerticalLineAtChar=100\nLineNumbers=2\n\
             AutoSaveInterval=30000\n[Interaction Options]\nTripleClickMode=1\nRightClickPasteMode=2\n\
             CopyTextAsHTML=false\n",
        );
        let migration = read_with_defaults(&home).unwrap();
        let value = |key: &str| setting(&migration, key);
        assert_eq!(value("Environment"), Some("EDITOR=vim"));
        assert_eq!(value("Margin"), Some("4"));
        assert_eq!(value("CenterContent"), Some("true"));
        assert_eq!(value("VerticalLine"), Some("100"));
        assert_eq!(value("LineNumbers"), Some("true"));
        assert_eq!(value("AutoSaveSeconds"), Some("30"));
        assert_eq!(value("TripleClick"), Some("forward"));
        assert_eq!(value("RightClickPaste"), Some("clipboard"));
        assert_eq!(value("CopyAsHtml"), Some("false"));
    }

    #[test]
    fn profile_options_are_taken_over() {
        let home = Home::new();
        home.write("config/konsolerc", "[Desktop Entry]\nDefaultProfile=Mine.profile\n");
        home.write(
            "data/konsole/Mine.profile",
            "[General]\nLocalTabTitleFormat=%n\nSilenceSeconds=30\n[Scrolling]\nScrollBarPosition=0\n[Cursor Options]\nBlinkingCursorEnabled=true\n",
        );
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(setting(&migration, "TitleFormat"), Some("%n"));
        assert_eq!(setting(&migration, "SilenceSeconds"), Some("30"));
        assert_eq!(setting(&migration, "ScrollBarPosition"), Some("left"));
        assert_eq!(setting(&migration, "BlinkingCursor"), Some("true"));
    }

    #[test]
    fn unlimited_history_is_mapped_to_the_maximum() {
        assert_eq!(history_lines(Some(&"2".to_string()), None).as_deref(), Some("1000000"));
        assert_eq!(history_lines(Some(&"0".to_string()), None).as_deref(), Some("0"));
    }

    #[test]
    fn names_cannot_leave_their_directory() {
        let home = Home::new();
        home.write("config/konsolerc", "[Desktop Entry]\nDefaultProfile=../../secret\n");
        home.write("config/yakuakerc", "[Appearance]\nSkin=../../other\n");
        home.write("secret", "[Appearance]\nFont=Evil,9\n");
        let migration = read_with_defaults(&home).unwrap();
        assert_eq!(setting(&migration, "FontFamily"), None);
        assert_eq!(setting(&migration, "Skin"), None);
    }

    #[test]
    fn colors_and_fonts_parse_or_fail_cleanly() {
        assert_eq!(color_name("#123456").as_deref(), Some("#123456"));
        assert_eq!(color_name("nonsense"), None);
        assert_eq!(font_parts("Hack"), None);
        assert_eq!(font_parts("Hack,-1"), None);
    }
}
