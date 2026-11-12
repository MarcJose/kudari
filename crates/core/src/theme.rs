// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The colors of a desktop theme for the window border and the terminals.

use crate::colors::{ColorScheme, parse_color};
use crate::services::Themes;
use crate::settings::THEME_PREFIX;

/// Colors as `#rrggbb`.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub header_background: String,
    pub header_foreground: String,
    pub window_background: String,
    pub window_foreground: String,
    pub highlight: String,
    pub highlight_text: String,
    pub negative: String,
    pub view_background: String,
    pub view_foreground: String,
    pub positive: String,
    pub neutral: String,
    pub link: String,
    pub visited: String,
    pub active: String,
    pub inactive: String,
}

/// The WCAG contrast that normal text needs.
const MINIMUM_CONTRAST: f64 = 4.5;

fn luminance(color: &str) -> f64 {
    let channel = |range: std::ops::Range<usize>| {
        let value = color
            .get(range)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .unwrap_or_default();
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(1..3) + 0.7152 * channel(3..5) + 0.0722 * channel(5..7)
}

/// The WCAG contrast of two colors, from 1 to 21.
fn contrast(first: &str, second: &str) -> f64 {
    let (first, second) = (luminance(first), luminance(second));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

/// Moves `color` the given percentage of the way towards `other`.
fn mix(color: &str, other: &str, percent: u32) -> String {
    let channel = |range: std::ops::Range<usize>| {
        let value = |color: &str| {
            let hex = color.get(range.clone());
            hex.and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .map_or(0, u32::from)
        };
        (value(color) * (100 - percent) + value(other) * percent) / 100
    };
    format!("#{:02x}{:02x}{:02x}", channel(1..3), channel(3..5), channel(5..7))
}

impl Palette {
    /// The color moved towards the text color until it can be read on the terminal background.
    fn readable(&self, color: &str) -> String {
        (0..=10)
            .map(|step| mix(color, &self.view_foreground, step * 10))
            .find(|mixed| contrast(mixed, &self.view_background) >= MINIMUM_CONTRAST)
            .unwrap_or_else(|| self.view_foreground.clone())
    }
}

/// What Kudari draws itself around the terminals, as `#rrggbb`: the border of the window.
/// The tab bar and its buttons are Plasma's own controls and take their colors from the system.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowColors {
    pub border: String,
}

impl From<&Palette> for WindowColors {
    fn from(palette: &Palette) -> Self {
        Self {
            border: palette.highlight.clone(),
        }
    }
}

/// Breeze, for the moment before the theme is known or should it not be readable.
impl Default for WindowColors {
    fn default() -> Self {
        Self {
            border: "#3daee9".to_string(),
        }
    }
}

/// The terminal colors of a palette. The ANSI colors from red to cyan are what the theme's
/// colors mean in applications. A theme has no black and white, so these are the darker and
/// the lighter end of its text and background; they stay that way even if they cannot be read
/// on the background, because programs draw black on yellow and white on blue with them.
fn terminal_colors(palette: &Palette) -> Option<ColorScheme> {
    let parse = parse_color;
    let (text, background) = (&palette.view_foreground, &palette.view_background);
    let (black, white) = if luminance(text) < luminance(background) {
        (text, background)
    } else {
        (background, text)
    };
    let hues = [
        &palette.negative,
        &palette.positive,
        &palette.neutral,
        &palette.link,
        &palette.visited,
        &palette.active,
    ]
    .map(|hue| palette.readable(hue));

    let mut scheme = ColorScheme {
        background: parse(background)?,
        foreground: parse(text)?,
        foreground_intense: parse(text)?,
        selection_background: parse(&palette.highlight)?,
        selection_text: parse(&palette.highlight_text)?,
        ..ColorScheme::default()
    };
    let colors = std::iter::once(black.clone())
        .chain(hues)
        .chain(std::iter::once(mix(white, black, 15)));
    let slots = scheme
        .normal
        .iter_mut()
        .zip(scheme.intense.iter_mut())
        .zip(scheme.faint.iter_mut());
    for (((normal, intense), faint), color) in slots.zip(colors) {
        *normal = parse(&color)?;
        *intense = parse(&mix(&color, text, 30))?;
        *faint = parse(&mix(&color, background, 33))?;
    }
    let [bright_black, .., bright_white] = &mut scheme.intense;
    *bright_black = parse(&palette.readable(&palette.inactive))?;
    *bright_white = parse(white)?;
    Some(scheme)
}

fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
}

/// Reads 15 colors separated by commas, in the order of the fields of [`Palette`].
#[must_use]
pub fn parse_palette(text: &str) -> Option<Palette> {
    let mut parts = text.split(',').filter(|part| is_color(part)).map(str::to_string);
    Some(Palette {
        header_background: parts.next()?,
        header_foreground: parts.next()?,
        window_background: parts.next()?,
        window_foreground: parts.next()?,
        highlight: parts.next()?,
        highlight_text: parts.next()?,
        negative: parts.next()?,
        view_background: parts.next()?,
        view_foreground: parts.next()?,
        positive: parts.next()?,
        neutral: parts.next()?,
        link: parts.next()?,
        visited: parts.next()?,
        active: parts.next()?,
        inactive: parts.next()?,
    })
}

/// The theme that a `Skin` setting names, `None` for the active one.
fn theme_of(skin: &str) -> Option<&str> {
    skin.strip_prefix(THEME_PREFIX)
}

/// The colors for a `Skin` setting.
#[must_use]
pub fn palette(themes: &dyn Themes, skin: &str) -> Option<Palette> {
    themes.palette(theme_of(skin))
}

/// The window colors for a `Skin` setting.
#[must_use]
pub fn window_colors(themes: &dyn Themes, skin: &str) -> Option<WindowColors> {
    palette(themes, skin).as_ref().map(WindowColors::from)
}

/// The terminal colors for a `Skin` setting.
#[must_use]
pub fn terminal_scheme(themes: &dyn Themes, skin: &str) -> Option<ColorScheme> {
    terminal_colors(&palette(themes, skin)?)
}

/// The installed themes as `Skin` setting and name. The active theme, [`SYSTEM_SKIN`](crate::settings::SYSTEM_SKIN), comes
/// on top of them.
#[must_use]
pub fn skins(themes: &dyn Themes) -> Vec<(String, String)> {
    themes
        .installed()
        .into_iter()
        .map(|(id, name)| (format!("{THEME_PREFIX}{id}"), name))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette {
            header_background: "#101010".into(),
            header_foreground: "#202020".into(),
            window_background: "#303030".into(),
            window_foreground: "#404040".into(),
            highlight: "#505050".into(),
            highlight_text: "#606060".into(),
            negative: "#707070".into(),
            view_background: "#808080".into(),
            view_foreground: "#909090".into(),
            positive: "#a0a0a0".into(),
            neutral: "#b0b0b0".into(),
            link: "#c0c0c0".into(),
            visited: "#d0d0d0".into(),
            active: "#e0e0e0".into(),
            inactive: "#f0f0f0".into(),
        }
    }

    fn breeze_light() -> Palette {
        Palette {
            negative: "#da4453".into(),
            positive: "#27ae60".into(),
            neutral: "#f67400".into(),
            link: "#2980b9".into(),
            visited: "#9b59b6".into(),
            active: "#3daee9".into(),
            inactive: "#707d8a".into(),
            view_background: "#ffffff".into(),
            view_foreground: "#232629".into(),
            ..palette()
        }
    }

    #[test]
    fn the_terminal_gets_the_colors_of_the_theme() {
        let palette = Palette {
            link: "#0000ff".into(),
            ..breeze_light()
        };
        let scheme = terminal_colors(&palette).unwrap();
        assert_eq!(scheme.background, parse_color("#ffffff").unwrap());
        assert_eq!(scheme.foreground, parse_color("#232629").unwrap());
        assert_eq!(scheme.normal[4], parse_color("#0000ff").unwrap());
        assert_eq!(scheme.normal[0], scheme.foreground);
        assert_eq!(scheme.intense[0], parse_color("#687480").unwrap());
        assert_eq!(scheme.normal[7], parse_color("#dedede").unwrap());
        assert_eq!(scheme.intense[7], scheme.background);
    }

    #[test]
    fn black_and_white_do_not_swap_on_a_dark_theme() {
        let dark = Palette {
            view_background: "#141618".into(),
            view_foreground: "#fcfcfc".into(),
            ..breeze_light()
        };
        let scheme = terminal_colors(&dark).unwrap();
        assert_eq!(scheme.normal[0], scheme.background);
        assert_eq!(scheme.intense[7], scheme.foreground);
    }

    #[test]
    fn every_hue_of_a_light_theme_can_be_read() {
        let light = breeze_light();
        for hue in [&light.positive, &light.neutral, &light.link, &light.active] {
            assert!(contrast(hue, &light.view_background) < MINIMUM_CONTRAST, "{hue}");
            assert!(contrast(&light.readable(hue), &light.view_background) >= MINIMUM_CONTRAST);
        }
    }

    #[test]
    fn a_readable_color_stays_as_it_is() {
        assert_eq!(breeze_light().readable("#0000ff"), "#0000ff");
    }

    #[test]
    fn a_theme_without_contrast_falls_back_to_its_text_color() {
        let flat = Palette {
            view_background: "#808080".into(),
            view_foreground: "#808080".into(),
            ..palette()
        };
        assert_eq!(flat.readable("#818181"), "#808080");
    }

    #[test]
    fn mixing_goes_from_one_color_to_the_other() {
        assert_eq!(mix("#000000", "#ffffff", 0), "#000000");
        assert_eq!(mix("#000000", "#ffffff", 100), "#ffffff");
        assert_eq!(mix("#102030", "#203040", 50), "#182838");
    }

    #[test]
    fn the_border_takes_the_highlight_color() {
        assert_eq!(WindowColors::from(&palette()).border, "#505050");
    }

    struct Installed;

    impl Themes for Installed {
        fn palette(&self, theme: Option<&str>) -> Option<Palette> {
            match theme {
                None => Some(breeze_light()),
                Some("org.kde.breezedark.desktop") => Some(Palette {
                    highlight: "#3daee9".into(),
                    ..palette()
                }),
                Some(_) => None,
            }
        }

        fn installed(&self) -> Vec<(String, String)> {
            vec![("org.kde.breezedark.desktop".into(), "Breeze Dark".into())]
        }
    }

    #[test]
    fn a_skin_names_the_active_or_an_installed_theme() {
        assert_eq!(super::palette(&Installed, "system"), Some(breeze_light()));
        let dark = window_colors(&Installed, "theme:org.kde.breezedark.desktop").unwrap();
        assert_eq!(dark.border, "#3daee9");
        assert_eq!(window_colors(&Installed, "theme:gone"), None);
        assert!(terminal_scheme(&Installed, "system").is_some());
        assert_eq!(terminal_scheme(&Installed, "theme:gone"), None);
    }

    #[test]
    fn installed_themes_become_skins() {
        assert_eq!(
            skins(&Installed),
            [(
                "theme:org.kde.breezedark.desktop".to_string(),
                "Breeze Dark".to_string()
            )]
        );
    }

    #[test]
    fn random_colors_do_no_harm() {
        for input in crate::fuzz::inputs(&["#808080", "#fff", "#1234567"]) {
            let _ = breeze_light().readable(&input);
            let _ = mix(&input, "#ffffff", 50);
            let _ = contrast(&input, "#000000");
        }
    }
}
