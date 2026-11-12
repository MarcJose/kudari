// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Application settings. Keys and defaults match Yakuake's, so that its settings carry over.

/// Where the settings live: `KConfig` on KDE Plasma, a file or a table elsewhere.
pub trait Store {
    /// The value of `key` in `group`, or `default` if it is not set.
    fn read(&self, group: &str, key: &str, default: &str) -> String;
    fn write(&self, group: &str, key: &str, value: &str);
}

/// The web search without a setting: a search engine that does not track who searches.
pub const DEFAULT_WEB_SEARCH: &str = "https://duckduckgo.com/?q=%s";

/// The `Skin` setting that follows the colors of the desktop.
pub const SYSTEM_SKIN: &str = "system";
/// Prefix of the `Skin` settings for one particular theme of the desktop.
pub const THEME_PREFIX: &str = "theme:";

/// Whether a `Skin` setting takes its colors from the desktop, as all skins do now.
#[must_use]
pub fn is_themed(id: &str) -> bool {
    id == SYSTEM_SKIN || id.starts_with(THEME_PREFIX)
}

/// Reads typed values from a store.
struct Source<'a>(&'a dyn Store);

impl Source<'_> {
    fn text(&self, group: &str, key: &str, default: &str) -> String {
        self.0.read(group, key, default)
    }

    fn boolean(&self, group: &str, key: &str, default: bool) -> bool {
        self.text(group, key, if default { "true" } else { "false" }) == "true"
    }

    fn int(&self, group: &str, key: &str, default: i32, min: i32, max: i32) -> i32 {
        self.text(group, key, &default.to_string())
            .parse::<i32>()
            .unwrap_or(default)
            .clamp(min, max)
    }

    fn float(&self, group: &str, key: &str, default: f64) -> f64 {
        self.text(group, key, &default.to_string()).parse().unwrap_or(default)
    }
}

/// Settings that can be changed at runtime, with their configuration group.
const KEYS: [(&str, &str); 106] = [
    ("Screen", "Window"),
    ("Width", "Window"),
    ("Height", "Window"),
    ("Position", "Window"),
    ("KeepOpen", "Window"),
    ("KeepOpenAfterLastSessionCloses", "Window"),
    ("ToggleToFocus", "Window"),
    ("ShowTabBar", "Window"),
    ("DynamicTabTitles", "Window"),
    ("ShowSystrayIcon", "Window"),
    ("FocusFollowsMouse", "Behavior"),
    ("OpenAfterStart", "Behavior"),
    ("RememberFullscreen", "Behavior"),
    ("Skin", "Appearance"),
    ("Translucency", "Appearance"),
    ("BackgroundColor", "Appearance"),
    ("BackgroundColorOpacity", "Appearance"),
    ("TerminalHighlightOnManualActivation", "Appearance"),
    ("HideSkinBorders", "Appearance"),
    ("ColorScheme", "Appearance"),
    ("ConfirmQuit", "Dialogs"),
    ("AllowRemoteCommands", "Security"),
    ("AllowAgentReading", "Security"),
    ("AllowAgentControl", "Security"),
    ("ConfirmAgentCommands", "Security"),
    ("FontFamily", "Terminal"),
    ("FontSize", "Terminal"),
    ("HistoryLines", "Terminal"),
    ("CursorShape", "Terminal"),
    ("Opacity", "Terminal"),
    ("Blur", "Terminal"),
    ("Command", "Terminal"),
    ("Shell", "Terminal"),
    ("ShellIntegration", "Terminal"),
    ("Directory", "Terminal"),
    ("TitleFormat", "Terminal"),
    ("ScrollBarPosition", "Terminal"),
    ("SilenceSeconds", "Terminal"),
    ("BellMode", "Terminal"),
    ("DimInactive", "Terminal"),
    ("ShowSizeHint", "Terminal"),
    ("WordSeparators", "Terminal"),
    ("WheelZoom", "Terminal"),
    ("AlternateScrolling", "Terminal"),
    ("MiddleClickPaste", "Terminal"),
    ("AutoCopySelection", "Terminal"),
    ("LineSpacing", "Terminal"),
    ("BoldIntense", "Terminal"),
    ("BlinkingCursor", "Terminal"),
    ("KittyKeyboard", "Terminal"),
    ("Margin", "Terminal"),
    ("CenterContent", "Terminal"),
    ("VerticalLine", "Terminal"),
    ("LineNumbers", "Terminal"),
    ("BadgeEnabled", "Terminal"),
    ("BadgeText", "Terminal"),
    ("BadgeColor", "Terminal"),
    ("BadgeFontFamily", "Terminal"),
    ("BadgeFontSize", "Terminal"),
    ("BadgeTextOnly", "Terminal"),
    ("BadgeTransparency", "Terminal"),
    ("TabColor", "Terminal"),
    ("TabActivityColor", "Terminal"),
    ("RemoteTitleFormat", "Terminal"),
    ("Environment", "Terminal"),
    ("CursorColor", "Terminal"),
    ("CursorTextColor", "Terminal"),
    ("BorderWhenActive", "Terminal"),
    ("FocusBorderColor", "Terminal"),
    ("DimValue", "Terminal"),
    ("AntiAliasFonts", "Terminal"),
    ("FontHinting", "Terminal"),
    ("TripleClick", "Terminal"),
    ("TrimLeadingSpaces", "Terminal"),
    ("TrimTrailingSpaces", "Terminal"),
    ("ClearSelectionOnCopy", "Terminal"),
    ("CopyAsHtml", "Terminal"),
    ("RightClickPaste", "Terminal"),
    ("OpenLinksByClick", "Terminal"),
    ("UnderlineLinks", "Terminal"),
    ("UnderlineFiles", "Terminal"),
    ("MouseTracking", "Terminal"),
    ("EditorCommand", "Terminal"),
    ("WebSearchUrl", "Terminal"),
    ("ScrollFullPage", "Terminal"),
    ("AutoSaveSeconds", "Terminal"),
    ("FlowControl", "Terminal"),
    ("EscapedLinks", "Terminal"),
    ("DragNeedsCtrl", "Terminal"),
    ("KeyBindings", "Terminal"),
    ("UrlHintsModifier", "Terminal"),
    ("ReverseUrlHints", "Terminal"),
    ("HighlightScrolledLines", "Terminal"),
    ("MarkerColor", "Terminal"),
    ("MarkerSize", "Terminal"),
    ("SearchLineColor", "Terminal"),
    ("SearchLineOpacity", "Terminal"),
    ("ColorFilter", "Terminal"),
    ("EmojiFont", "Terminal"),
    ("FontLineCharacters", "Terminal"),
    ("FontBrailleCharacters", "Terminal"),
    ("AnimatedCursor", "Terminal"),
    ("InvertSelection", "Terminal"),
    ("LinkSchemes", "Terminal"),
    ("DropUrlsAsText", "Terminal"),
    ("InheritDirectory", "Terminal"),
];

/// Writes a setting. Returns false for keys that do not exist.
pub fn store(store: &dyn Store, key: &str, value: &str) -> bool {
    match KEYS.iter().find(|(name, _)| *name == key) {
        Some((_, group)) => {
            store.write(group, key, value);
            true
        }
        None => false,
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub screen: i32,
    pub width: i32,
    pub height: i32,
    pub position: i32,
    pub keep_open: bool,
    pub keep_open_after_last_session_closes: bool,
    pub toggle_to_focus: bool,
    pub show_tab_bar: bool,
    pub dynamic_tab_titles: bool,
    pub show_systray_icon: bool,
    pub focus_follows_mouse: bool,
    pub open_after_start: bool,
    pub remember_fullscreen: bool,
    pub skin: String,
    pub translucency: bool,
    pub background_color: String,
    pub background_color_opacity: i32,
    pub terminal_highlight_on_manual_activation: bool,
    pub hide_skin_borders: bool,
    /// Light or dark, where the front end chooses that itself: `system`, `light` or `dark`.
    pub color_scheme: String,
    pub terminal_highlight_opacity: f64,
    pub terminal_highlight_duration: i32,
    pub keyboard_block_color: String,
    pub keyboard_block_opacity: f64,
    pub keyboard_block_duration: i32,
    pub confirm_quit: bool,
    /// Lets the D-Bus method `runCommand` type into terminals. Any program of the
    /// same user could use it to run commands, so it stays off unless enabled.
    pub allow_remote_commands: bool,
    /// Lets AI agents read the text of the terminals through `kudari mcp`. Off by default:
    /// any program of the same user can reach the interface.
    pub allow_agent_reading: bool,
    /// Lets AI agents type into terminals and open and close their own sessions.
    pub allow_agent_control: bool,
    /// Leaves the Return of an agent's command to the user, and lets agents type only
    /// while the shell itself waits for input.
    pub confirm_agent_commands: bool,
    /// Empty means the system's fixed-width font.
    pub font_family: String,
    /// Zero means the size of the system's fixed-width font.
    pub font_size: i32,
    pub history_lines: i32,
    /// `block`, `underline` or `beam`.
    pub cursor_shape: String,
    /// Opacity of the terminal background in percent.
    pub opacity: i32,
    pub blur: bool,
    /// Program to start instead of the login shell, with its arguments.
    pub command: String,
    /// The shell new tabs start without `command`, by the id of [`crate::shells::Shell`].
    /// Empty means the default of the platform.
    pub shell: String,
    /// Lets the shells of Windows mark their prompt and report their directory.
    pub shell_integration: bool,
    /// Directory for new terminals. Empty means the directory of the active terminal.
    pub directory: String,
    /// Konsole's title format; see `terminal::format_title`.
    pub title_format: String,
    /// `right`, `left` or `hidden`.
    pub scroll_bar_position: String,
    pub silence_seconds: i32,
    /// `notify`, `visual` or `none`.
    pub bell_mode: String,
    pub dim_inactive: bool,
    pub show_size_hint: bool,
    /// Characters that end a word on double click. Empty means the default set.
    pub word_separators: String,
    pub wheel_zoom: bool,
    pub alternate_scrolling: bool,
    /// `selection`, `clipboard` or `none`.
    pub middle_click_paste: String,
    pub auto_copy_selection: bool,
    pub line_spacing: i32,
    pub bold_intense: bool,
    pub blinking_cursor: bool,
    pub kitty_keyboard: bool,
    pub margin: i32,
    pub center_content: bool,
    pub vertical_line: i32,
    pub line_numbers: bool,
    pub badge_enabled: bool,
    pub badge_text: String,
    pub badge_color: String,
    pub badge_font_family: String,
    pub badge_font_size: i32,
    pub badge_text_only: bool,
    pub badge_transparency: i32,
    pub tab_color: String,
    pub tab_activity_color: String,
    pub remote_title_format: String,
    pub environment: String,
    pub cursor_color: String,
    pub cursor_text_color: String,
    pub border_when_active: bool,
    pub focus_border_color: String,
    pub dim_value: i32,
    pub anti_alias_fonts: bool,
    pub font_hinting: bool,
    pub triple_click: String,
    pub trim_leading_spaces: bool,
    pub trim_trailing_spaces: bool,
    pub clear_selection_on_copy: bool,
    pub copy_as_html: bool,
    pub right_click_paste: String,
    pub open_links_by_click: bool,
    pub underline_links: bool,
    pub underline_files: bool,
    pub mouse_tracking: bool,
    pub editor_command: String,
    /// The address of a web search, with `%s` for the words, where the platform does not
    /// choose the search engine itself as KDE does.
    pub web_search_url: String,
    pub scroll_full_page: bool,
    pub auto_save_seconds: i32,
    pub flow_control: bool,
    pub escaped_links: bool,
    pub drag_needs_ctrl: bool,
    pub key_bindings: String,
    pub url_hints_modifier: String,
    pub reverse_url_hints: bool,
    pub highlight_scrolled_lines: bool,
    pub marker_color: String,
    pub marker_size: i32,
    pub search_line_color: String,
    pub search_line_opacity: i32,
    pub color_filter: bool,
    pub emoji_font: String,
    pub font_line_characters: bool,
    pub font_braille_characters: bool,
    pub animated_cursor: bool,
    pub invert_selection: bool,
    pub link_schemes: String,
    pub drop_urls_as_text: bool,
    pub inherit_directory: bool,
    pub first_run: bool,
}

/// Records that the first-run dialog has been shown.
pub fn mark_first_run_done(store: &dyn Store) {
    store.write("Dialogs", "FirstRun", "false");
}

impl Settings {
    /// Whether the window blurs what shines through it.
    #[must_use]
    pub fn blurs_behind(&self) -> bool {
        self.opacity < 100 && self.blur
    }

    pub fn load(store: &dyn Store) -> Self {
        Self::from_source(&Source(store))
    }

    fn from_source(source: &Source) -> Self {
        Self {
            screen: source.int("Window", "Screen", 0, 0, i32::MAX),
            width: source.int("Window", "Width", 90, 10, 100),
            height: source.int("Window", "Height", 50, 10, 100),
            position: source.int("Window", "Position", 50, 0, 100),
            keep_open: source.boolean("Window", "KeepOpen", true),
            keep_open_after_last_session_closes: source.boolean("Window", "KeepOpenAfterLastSessionCloses", false),
            toggle_to_focus: source.boolean("Window", "ToggleToFocus", false),
            show_tab_bar: source.boolean("Window", "ShowTabBar", true),
            dynamic_tab_titles: source.boolean("Window", "DynamicTabTitles", true),
            show_systray_icon: source.boolean("Window", "ShowSystrayIcon", true),
            focus_follows_mouse: source.boolean("Behavior", "FocusFollowsMouse", false),
            open_after_start: source.boolean("Behavior", "OpenAfterStart", false),
            remember_fullscreen: source.boolean("Behavior", "RememberFullscreen", false),
            skin: Some(source.text("Appearance", "Skin", SYSTEM_SKIN))
                .filter(|skin| is_themed(skin))
                .unwrap_or_else(|| SYSTEM_SKIN.to_string()),
            translucency: source.boolean("Appearance", "Translucency", false),
            background_color: source.text("Appearance", "BackgroundColor", "#000000"),
            background_color_opacity: source.int("Appearance", "BackgroundColorOpacity", 50, 0, 100),
            terminal_highlight_on_manual_activation: source.boolean(
                "Appearance",
                "TerminalHighlightOnManualActivation",
                false,
            ),
            hide_skin_borders: source.boolean("Appearance", "HideSkinBorders", false),
            color_scheme: source.text("Appearance", "ColorScheme", "system"),
            terminal_highlight_opacity: source.float("Appearance", "TerminalHighlightOpacity", 0.4),
            terminal_highlight_duration: source.int("Appearance", "TerminalHighlightDuration", 250, 0, 86_400_000),
            keyboard_block_color: source.text("Appearance", "KeyboardInputBlockIndicatorColor", "#FF0000"),
            keyboard_block_opacity: source.float("Appearance", "KeyboardInputBlockIndicatorOpacity", 0.5),
            keyboard_block_duration: source.int(
                "Appearance",
                "KeyboardInputBlockIndicatorDuration",
                250,
                0,
                86_400_000,
            ),
            confirm_quit: source.boolean("Dialogs", "ConfirmQuit", true),
            allow_remote_commands: source.boolean("Security", "AllowRemoteCommands", false),
            allow_agent_reading: source.boolean("Security", "AllowAgentReading", false),
            allow_agent_control: source.boolean("Security", "AllowAgentControl", false),
            confirm_agent_commands: source.boolean("Security", "ConfirmAgentCommands", true),
            font_family: source.text("Terminal", "FontFamily", ""),
            font_size: source.int("Terminal", "FontSize", 0, 0, 200),
            history_lines: source.int("Terminal", "HistoryLines", 1000, 0, 1_000_000),
            cursor_shape: source.text("Terminal", "CursorShape", "block"),
            opacity: source.int("Terminal", "Opacity", 100, 10, 100),
            blur: source.boolean("Terminal", "Blur", true),
            command: source.text("Terminal", "Command", ""),
            shell: source.text("Terminal", "Shell", ""),
            shell_integration: source.boolean("Terminal", "ShellIntegration", true),
            directory: source.text("Terminal", "Directory", ""),
            title_format: source.text("Terminal", "TitleFormat", "%d : %n"),
            scroll_bar_position: source.text("Terminal", "ScrollBarPosition", "right"),
            silence_seconds: source.int("Terminal", "SilenceSeconds", 10, 1, 3600),
            bell_mode: source.text("Terminal", "BellMode", "notify"),
            dim_inactive: source.boolean("Terminal", "DimInactive", false),
            show_size_hint: source.boolean("Terminal", "ShowSizeHint", true),
            word_separators: source.text("Terminal", "WordSeparators", ""),
            wheel_zoom: source.boolean("Terminal", "WheelZoom", true),
            alternate_scrolling: source.boolean("Terminal", "AlternateScrolling", true),
            middle_click_paste: source.text("Terminal", "MiddleClickPaste", "selection"),
            auto_copy_selection: source.boolean("Terminal", "AutoCopySelection", true),
            line_spacing: source.int("Terminal", "LineSpacing", 0, 0, 20),
            bold_intense: source.boolean("Terminal", "BoldIntense", true),
            blinking_cursor: source.boolean("Terminal", "BlinkingCursor", false),
            kitty_keyboard: source.boolean("Terminal", "KittyKeyboard", false),
            margin: source.int("Terminal", "Margin", 0, 0, 50),
            center_content: source.boolean("Terminal", "CenterContent", false),
            vertical_line: source.int("Terminal", "VerticalLine", 0, 0, 500),
            line_numbers: source.boolean("Terminal", "LineNumbers", false),
            badge_enabled: source.boolean("Terminal", "BadgeEnabled", false),
            badge_text: source.text("Terminal", "BadgeText", ""),
            badge_color: source.text("Terminal", "BadgeColor", "#ffffff"),
            badge_font_family: source.text("Terminal", "BadgeFontFamily", ""),
            badge_font_size: source.int("Terminal", "BadgeFontSize", 0, 0, 200),
            badge_text_only: source.boolean("Terminal", "BadgeTextOnly", true),
            badge_transparency: source.int("Terminal", "BadgeTransparency", 180, 0, 255),
            tab_color: source.text("Terminal", "TabColor", ""),
            tab_activity_color: source.text("Terminal", "TabActivityColor", ""),
            remote_title_format: source.text("Terminal", "RemoteTitleFormat", "(%u) %H"),
            environment: source.text("Terminal", "Environment", ""),
            cursor_color: source.text("Terminal", "CursorColor", ""),
            cursor_text_color: source.text("Terminal", "CursorTextColor", ""),
            border_when_active: source.boolean("Terminal", "BorderWhenActive", false),
            focus_border_color: source.text("Terminal", "FocusBorderColor", "#3daee9"),
            dim_value: source.int("Terminal", "DimValue", 77, 0, 255),
            anti_alias_fonts: source.boolean("Terminal", "AntiAliasFonts", true),
            font_hinting: source.boolean("Terminal", "FontHinting", false),
            triple_click: source.text("Terminal", "TripleClick", "line"),
            trim_leading_spaces: source.boolean("Terminal", "TrimLeadingSpaces", false),
            trim_trailing_spaces: source.boolean("Terminal", "TrimTrailingSpaces", false),
            clear_selection_on_copy: source.boolean("Terminal", "ClearSelectionOnCopy", false),
            copy_as_html: source.boolean("Terminal", "CopyAsHtml", true),
            right_click_paste: source.text("Terminal", "RightClickPaste", "none"),
            open_links_by_click: source.boolean("Terminal", "OpenLinksByClick", false),
            underline_links: source.boolean("Terminal", "UnderlineLinks", true),
            underline_files: source.boolean("Terminal", "UnderlineFiles", false),
            mouse_tracking: source.boolean("Terminal", "MouseTracking", true),
            editor_command: source.text("Terminal", "EditorCommand", ""),
            web_search_url: source.text("Terminal", "WebSearchUrl", DEFAULT_WEB_SEARCH),
            scroll_full_page: source.boolean("Terminal", "ScrollFullPage", false),
            auto_save_seconds: source.int("Terminal", "AutoSaveSeconds", 10, 1, 3600),
            flow_control: source.boolean("Terminal", "FlowControl", true),
            escaped_links: source.boolean("Terminal", "EscapedLinks", true),
            drag_needs_ctrl: source.boolean("Terminal", "DragNeedsCtrl", true),
            key_bindings: source.text("Terminal", "KeyBindings", ""),
            url_hints_modifier: source.text("Terminal", "UrlHintsModifier", "none"),
            reverse_url_hints: source.boolean("Terminal", "ReverseUrlHints", false),
            highlight_scrolled_lines: source.boolean("Terminal", "HighlightScrolledLines", true),
            marker_color: source.text("Terminal", "MarkerColor", "#ff0000"),
            marker_size: source.int("Terminal", "MarkerSize", 2, 1, 50),
            search_line_color: source.text("Terminal", "SearchLineColor", "#ffff00"),
            search_line_opacity: source.int("Terminal", "SearchLineOpacity", 100, 0, 100),
            color_filter: source.boolean("Terminal", "ColorFilter", true),
            emoji_font: source.text("Terminal", "EmojiFont", ""),
            font_line_characters: source.boolean("Terminal", "FontLineCharacters", false),
            font_braille_characters: source.boolean("Terminal", "FontBrailleCharacters", true),
            animated_cursor: source.boolean("Terminal", "AnimatedCursor", false),
            invert_selection: source.boolean("Terminal", "InvertSelection", false),
            link_schemes: source.text("Terminal", "LinkSchemes", "http://;https://;mailto:"),
            drop_urls_as_text: source.boolean("Terminal", "DropUrlsAsText", true),
            inherit_directory: source.boolean("Terminal", "InheritDirectory", true),
            first_run: source.boolean("Dialogs", "FirstRun", true),
        }
    }

    /// The values shown in the settings dialog, by configuration key.
    #[must_use]
    pub fn to_config_json(&self) -> serde_json::Value {
        serde_json::json!({
            "Screen": self.screen,
            "Width": self.width,
            "Height": self.height,
            "Position": self.position,
            "KeepOpen": self.keep_open,
            "KeepOpenAfterLastSessionCloses": self.keep_open_after_last_session_closes,
            "ToggleToFocus": self.toggle_to_focus,
            "ShowTabBar": self.show_tab_bar,
            "DynamicTabTitles": self.dynamic_tab_titles,
            "ShowSystrayIcon": self.show_systray_icon,
            "FocusFollowsMouse": self.focus_follows_mouse,
            "OpenAfterStart": self.open_after_start,
            "RememberFullscreen": self.remember_fullscreen,
            "Skin": self.skin,
            "Translucency": self.translucency,
            "BackgroundColor": self.background_color,
            "BackgroundColorOpacity": self.background_color_opacity,
            "TerminalHighlightOnManualActivation": self.terminal_highlight_on_manual_activation,
            "HideSkinBorders": self.hide_skin_borders,
            "ColorScheme": self.color_scheme,
            "ConfirmQuit": self.confirm_quit,
            "AllowRemoteCommands": self.allow_remote_commands,
            "AllowAgentReading": self.allow_agent_reading,
            "AllowAgentControl": self.allow_agent_control,
            "ConfirmAgentCommands": self.confirm_agent_commands,
            "FontFamily": self.font_family,
            "FontSize": self.font_size,
            "HistoryLines": self.history_lines,
            "CursorShape": self.cursor_shape,
            "Opacity": self.opacity,
            "Blur": self.blur,
            "Command": self.command,
            "Shell": self.shell,
            "ShellIntegration": self.shell_integration,
            "Directory": self.directory,
            "TitleFormat": self.title_format,
            "ScrollBarPosition": self.scroll_bar_position,
            "SilenceSeconds": self.silence_seconds,
            "BellMode": self.bell_mode,
            "DimInactive": self.dim_inactive,
            "ShowSizeHint": self.show_size_hint,
            "WordSeparators": self.word_separators,
            "WheelZoom": self.wheel_zoom,
            "AlternateScrolling": self.alternate_scrolling,
            "MiddleClickPaste": self.middle_click_paste,
            "AutoCopySelection": self.auto_copy_selection,
            "LineSpacing": self.line_spacing,
            "BoldIntense": self.bold_intense,
            "BlinkingCursor": self.blinking_cursor,
            "KittyKeyboard": self.kitty_keyboard,
            "Margin": self.margin,
            "CenterContent": self.center_content,
            "VerticalLine": self.vertical_line,
            "LineNumbers": self.line_numbers,
            "BadgeEnabled": self.badge_enabled,
            "BadgeText": self.badge_text,
            "BadgeColor": self.badge_color,
            "BadgeFontFamily": self.badge_font_family,
            "BadgeFontSize": self.badge_font_size,
            "BadgeTextOnly": self.badge_text_only,
            "BadgeTransparency": self.badge_transparency,
            "TabColor": self.tab_color,
            "TabActivityColor": self.tab_activity_color,
            "RemoteTitleFormat": self.remote_title_format,
            "Environment": self.environment,
            "CursorColor": self.cursor_color,
            "CursorTextColor": self.cursor_text_color,
            "BorderWhenActive": self.border_when_active,
            "FocusBorderColor": self.focus_border_color,
            "DimValue": self.dim_value,
            "AntiAliasFonts": self.anti_alias_fonts,
            "FontHinting": self.font_hinting,
            "TripleClick": self.triple_click,
            "TrimLeadingSpaces": self.trim_leading_spaces,
            "TrimTrailingSpaces": self.trim_trailing_spaces,
            "ClearSelectionOnCopy": self.clear_selection_on_copy,
            "CopyAsHtml": self.copy_as_html,
            "RightClickPaste": self.right_click_paste,
            "OpenLinksByClick": self.open_links_by_click,
            "UnderlineLinks": self.underline_links,
            "UnderlineFiles": self.underline_files,
            "MouseTracking": self.mouse_tracking,
            "EditorCommand": self.editor_command,
            "WebSearchUrl": self.web_search_url,
            "ScrollFullPage": self.scroll_full_page,
            "AutoSaveSeconds": self.auto_save_seconds,
            "FlowControl": self.flow_control,
            "EscapedLinks": self.escaped_links,
            "DragNeedsCtrl": self.drag_needs_ctrl,
            "KeyBindings": self.key_bindings,
            "UrlHintsModifier": self.url_hints_modifier,
            "ReverseUrlHints": self.reverse_url_hints,
            "HighlightScrolledLines": self.highlight_scrolled_lines,
            "MarkerColor": self.marker_color,
            "MarkerSize": self.marker_size,
            "SearchLineColor": self.search_line_color,
            "SearchLineOpacity": self.search_line_opacity,
            "ColorFilter": self.color_filter,
            "EmojiFont": self.emoji_font,
            "FontLineCharacters": self.font_line_characters,
            "FontBrailleCharacters": self.font_braille_characters,
            "AnimatedCursor": self.animated_cursor,
            "InvertSelection": self.invert_selection,
            "LinkSchemes": self.link_schemes,
            "DropUrlsAsText": self.drop_urls_as_text,
            "InheritDirectory": self.inherit_directory,
        })
    }

    pub fn set_keep_open(&mut self, store: &dyn Store, keep_open: bool) {
        self.keep_open = keep_open;
        store.write("Window", "KeepOpen", if keep_open { "true" } else { "false" });
    }

    pub fn set_screen(&mut self, store: &dyn Store, screen: i32) {
        self.screen = screen;
        store.write("Window", "Screen", &screen.to_string());
    }

    pub fn set_width(&mut self, store: &dyn Store, width: i32) {
        self.width = width.clamp(10, 100);
        store.write("Window", "Width", &self.width.to_string());
    }

    pub fn set_height(&mut self, store: &dyn Store, height: i32) {
        self.height = height.clamp(10, 100);
        store.write("Window", "Height", &self.height.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    /// Settings in memory, by key.
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

    fn with(values: &[(&str, &str)]) -> Settings {
        let table = Table::default();
        for (key, value) in values {
            table.write("", key, value);
        }
        Settings::load(&table)
    }

    #[test]
    fn changes_reach_the_store() {
        let table = Table::default();
        let mut settings = Settings::load(&table);
        settings.set_keep_open(&table, true);
        settings.set_screen(&table, 2);
        settings.set_width(&table, 500);
        settings.set_height(&table, 30);
        assert!(store(&table, "Opacity", "80"));
        assert!(!store(&table, "NoSuchKey", "1"));
        mark_first_run_done(&table);
        let again = Settings::load(&table);
        assert!(again.keep_open && !again.first_run);
        assert_eq!((again.screen, again.width, again.height), (2, 100, 30));
        assert_eq!(table.read("", "Opacity", ""), "80");
    }

    #[test]
    fn defaults_suit_a_new_installation() {
        let settings = with(&[]);
        assert_eq!((settings.width, settings.height, settings.position), (90, 50, 50));
        assert_eq!(settings.skin, "system");
        assert_eq!(settings.title_format, "%d : %n");
        assert_eq!(settings.remote_title_format, "(%u) %H");
        assert!(settings.dynamic_tab_titles && settings.confirm_quit && settings.underline_links);
    }

    #[test]
    fn dangerous_features_start_switched_off() {
        let settings = with(&[]);
        assert!(!settings.allow_remote_commands);
        assert!(!settings.allow_agent_reading && !settings.allow_agent_control && settings.confirm_agent_commands);
        assert!(!settings.open_links_by_click);
        assert_eq!(settings.link_schemes, "http://;https://;mailto:");
        assert_eq!(settings.editor_command, "");
    }

    #[test]
    fn skins_of_older_versions_become_the_system_theme() {
        assert_eq!(with(&[("Skin", "plastik_dark")]).skin, "system");
        assert_eq!(with(&[("Skin", "default")]).skin, "system");
        assert_eq!(with(&[("Skin", "theme:org.kde.oxygen")]).skin, "theme:org.kde.oxygen");
    }

    #[test]
    fn numbers_stay_in_their_range() {
        let settings = with(&[
            ("Width", "5"),
            ("Height", "900"),
            ("SilenceSeconds", "-3"),
            ("DimValue", "1000"),
        ]);
        assert_eq!((settings.width, settings.height), (10, 100));
        assert_eq!((settings.silence_seconds, settings.dim_value), (1, 255));
    }

    #[test]
    fn broken_values_fall_back_to_the_default() {
        let settings = with(&[
            ("Width", "breit"),
            ("TerminalHighlightOpacity", "x"),
            ("KeepOpen", "yes"),
        ]);
        assert_eq!(settings.width, 90);
        assert!((settings.terminal_highlight_opacity - 0.4).abs() < f64::EPSILON);
        assert!(!settings.keep_open, "only `true` counts as true");
    }

    #[test]
    fn setting_keys_are_unique() {
        let names: HashSet<_> = KEYS.iter().map(|(name, _)| name).collect();
        assert_eq!(names.len(), KEYS.len());
    }

    #[test]
    fn the_settings_dialog_knows_every_key() {
        let config = with(&[]).to_config_json();
        let missing: Vec<_> = KEYS.iter().filter(|(name, _)| config.get(name).is_none()).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }
}
