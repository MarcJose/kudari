// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The colors of the terminals, which `theme` takes from Plasma.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

#[derive(Clone, Debug, PartialEq)]
pub struct ColorScheme {
    pub background: Rgb,
    pub foreground: Rgb,
    pub foreground_intense: Rgb,
    pub normal: [Rgb; 8],
    pub intense: [Rgb; 8],
    pub faint: [Rgb; 8],
    /// Selected text, in the theme's colors for selections.
    pub selection_background: Rgb,
    pub selection_text: Rgb,
}

/// Plain text until the theme is known, or if it cannot be read.
impl Default for ColorScheme {
    fn default() -> Self {
        let foreground = Rgb { r: 252, g: 252, b: 252 };
        Self {
            background: Rgb { r: 35, g: 38, b: 39 },
            foreground,
            foreground_intense: foreground,
            normal: [foreground; 8],
            intense: [foreground; 8],
            faint: [foreground; 8],
            selection_background: Rgb { r: 61, g: 174, b: 233 },
            selection_text: Rgb { r: 255, g: 255, b: 255 },
        }
    }
}

/// Faint text: a third of the way towards the background, like the faint colors of a theme.
/// It fades on light backgrounds as well as on dark ones.
#[must_use]
pub fn faded(color: Rgb, background: Rgb) -> Rgb {
    let mix = |channel: u8, under: u8| ((u16::from(channel) * 2 + u16::from(under)) / 3) as u8;
    Rgb {
        r: mix(color.r, background.r),
        g: mix(color.g, background.g),
        b: mix(color.b, background.b),
    }
}

/// Shared with Qt's render thread.
static ACTIVE: LazyLock<RwLock<Arc<ColorScheme>>> = LazyLock::new(RwLock::default);

pub fn active() -> Arc<ColorScheme> {
    ACTIVE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn set_scheme(scheme: ColorScheme) {
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(scheme);
}

/// A color written as `#rrggbb`.
#[must_use]
pub fn parse_color(text: &str) -> Option<Rgb> {
    let hex = text.strip_prefix('#').filter(|hex| hex.len() == 6 && hex.is_ascii())?;
    let part = |range| hex.get(range).and_then(|digits| u8::from_str_radix(digits, 16).ok());
    Some(Rgb {
        r: part(0..2)?,
        g: part(2..4)?,
        b: part(4..6)?,
    })
}

/// The color of a cell: what the program set, else the scheme.
#[must_use]
pub fn resolve(color: Color, overrides: &Colors, scheme: &ColorScheme) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Named(name) => overrides[name].unwrap_or_else(|| named_color(name, scheme)),
        Color::Indexed(index) => overrides[index as usize].unwrap_or_else(|| indexed_color(index, scheme)),
    }
}

/// Bold text uses the intense variant of the eight base colors and the foreground.
#[must_use]
pub fn brighten(color: Color, resolved: Rgb, overrides: &Colors, scheme: &ColorScheme) -> Rgb {
    match color {
        Color::Named(NamedColor::Foreground) => scheme.foreground_intense,
        Color::Named(name) if (name as usize) < 8 => resolve(Color::Indexed(name as u8 + 8), overrides, scheme),
        Color::Indexed(index) if index < 8 => resolve(Color::Indexed(index + 8), overrides, scheme),
        _ => resolved,
    }
}

/// The faint variant of a color: the scheme's, else a third of the way to the background.
#[must_use]
pub fn faint(color: Color, resolved: Rgb, scheme: &ColorScheme) -> Rgb {
    match color {
        Color::Named(name) => scheme
            .faint
            .get(name as usize)
            .copied()
            .unwrap_or_else(|| faded(resolved, scheme.background)),
        Color::Indexed(index) => scheme
            .faint
            .get(index as usize)
            .copied()
            .unwrap_or_else(|| faded(resolved, scheme.background)),
        Color::Spec(_) => faded(resolved, scheme.background),
    }
}

/// A named color in the scheme.
#[must_use]
pub fn named_color(name: NamedColor, scheme: &ColorScheme) -> Rgb {
    match name as usize {
        index @ 0..=7 => scheme.normal.get(index).copied().unwrap_or(scheme.foreground),
        index @ 8..=15 => scheme.intense.get(index - 8).copied().unwrap_or(scheme.foreground),
        257 => scheme.background,
        index @ 259..=266 => scheme.faint.get(index - 259).copied().unwrap_or(scheme.foreground),
        267 => scheme.foreground_intense,
        268 => faded(scheme.foreground, scheme.background),
        _ => scheme.foreground,
    }
}

/// One of the 256 colors: the scheme's sixteen, then xterm's cube and grays.
#[must_use]
pub fn indexed_color(index: u8, scheme: &ColorScheme) -> Rgb {
    const fn cube(step: u8) -> u8 {
        if step == 0 { 0 } else { 55 + step * 40 }
    }
    match index {
        0..=7 => scheme
            .normal
            .get(usize::from(index))
            .copied()
            .unwrap_or(scheme.foreground),
        8..=15 => scheme
            .intense
            .get(usize::from(index) - 8)
            .copied()
            .unwrap_or(scheme.foreground),
        16..=231 => {
            let value = index - 16;
            Rgb {
                r: cube(value / 36),
                g: cube(value / 6 % 6),
                b: cube(value % 6),
            }
        }
        232..=255 => {
            let level = 8 + (index - 232) * 10;
            Rgb {
                r: level,
                g: level,
                b: level,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faint_text_moves_towards_the_background() {
        let white = Rgb { r: 255, g: 255, b: 255 };
        let black = Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(faded(white, black), Rgb { r: 170, g: 170, b: 170 });
        assert_eq!(faded(black, white), Rgb { r: 85, g: 85, b: 85 });
    }

    #[test]
    fn the_color_cube_matches_xterm() {
        let scheme = active();
        assert_eq!(indexed_color(196, &scheme), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(indexed_color(232, &scheme), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(indexed_color(255, &scheme), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn colors_come_from_the_scheme_and_xterm() {
        let scheme = active();
        let overrides = Colors::default();
        assert_eq!(indexed_color(3, &scheme), scheme.normal[3]);
        assert_eq!(indexed_color(11, &scheme), scheme.intense[3]);
        assert_eq!(indexed_color(16, &scheme), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(indexed_color(231, &scheme), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(named_color(NamedColor::Red, &scheme), scheme.normal[1]);
        assert_eq!(named_color(NamedColor::BrightRed, &scheme), scheme.intense[1]);
        assert_eq!(named_color(NamedColor::Background, &scheme), scheme.background);
        assert_eq!(named_color(NamedColor::DimRed, &scheme), scheme.faint[1]);
        assert_eq!(
            named_color(NamedColor::BrightForeground, &scheme),
            scheme.foreground_intense
        );
        assert_eq!(
            named_color(NamedColor::DimForeground, &scheme),
            faded(scheme.foreground, scheme.background)
        );
        assert_eq!(named_color(NamedColor::Cursor, &scheme), scheme.foreground);
        let spec = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(resolve(Color::Spec(spec), &overrides, &scheme), spec);
    }

    #[test]
    fn bold_brightens_and_faint_dims() {
        let scheme = active();
        let overrides = Colors::default();
        let red = scheme.normal[1];
        let foreground = Color::Named(NamedColor::Foreground);
        assert_eq!(
            brighten(foreground, scheme.foreground, &overrides, &scheme),
            scheme.foreground_intense
        );
        assert_eq!(
            brighten(Color::Named(NamedColor::Red), red, &overrides, &scheme),
            scheme.intense[1]
        );
        assert_eq!(brighten(Color::Indexed(1), red, &overrides, &scheme), scheme.intense[1]);
        assert_eq!(brighten(Color::Indexed(100), red, &overrides, &scheme), red);
        assert_eq!(faint(Color::Named(NamedColor::Red), red, &scheme), scheme.faint[1]);
        assert_eq!(faint(Color::Indexed(1), red, &scheme), scheme.faint[1]);
        let spec = Rgb { r: 200, g: 0, b: 0 };
        let dimmed = faded(spec, scheme.background);
        assert_eq!(faint(Color::Spec(spec), spec, &scheme), dimmed);
        assert_eq!(faint(Color::Indexed(100), spec, &scheme), dimmed);
        assert_eq!(faint(foreground, spec, &scheme), dimmed);
    }
}
