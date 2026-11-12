// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What a terminal looks like, as a list of rectangles and texts in pixels. The core decides
//! colors, styles and shapes; a front end only paints the [`Paint`]s with its own fonts.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::Match;
use alacritty_terminal::vte::ansi::{CursorShape, Rgb};

use crate::colors::{self, ColorScheme, brighten, faint, resolve};
use crate::linedraw;
use crate::options::Options;

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the grid lies and how big its cells are, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// The top left corner of the grid.
    pub origin_x: f64,
    pub origin_y: f64,
    pub cell_width: f64,
    pub cell_height: f64,
    /// From the top of a cell to the baseline of its text.
    pub ascent: f64,
}

impl Geometry {
    /// The rectangle of `cells` cells from a column of a screen line.
    #[must_use]
    pub fn cell_rect(&self, column: i32, line: i32, cells: f64) -> Rect {
        Rect {
            x: self.origin_x + f64::from(column) * self.cell_width,
            y: self.origin_y + f64::from(line) * self.cell_height,
            width: cells * self.cell_width,
            height: self.cell_height,
        }
    }

    /// Where the text of a cell starts on its baseline, on whole pixels.
    #[must_use]
    pub fn baseline(&self, column: i32, line: i32) -> (i32, i32) {
        (
            (self.origin_x + f64::from(column) * self.cell_width) as i32,
            (self.origin_y + f64::from(line) * self.cell_height + self.ascent) as i32,
        )
    }
}

/// The variant of the terminal font a text needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
    /// An emoji, for the emoji font if one is set.
    pub emoji: bool,
}

/// One thing to paint, in the order of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Fill(Rect, Rgb),
    /// Text starting at a point of its baseline.
    Text {
        x: i32,
        y: i32,
        text: String,
        color: Rgb,
        font: FontStyle,
    },
}

/// What a frame depends on besides the terminal and its options.
pub struct Scene<'a> {
    pub geometry: Geometry,
    pub scheme: &'a ColorScheme,
    /// Bold text in the intense colors.
    pub bold_intense: bool,
    pub focused: bool,
    /// The cursor is in the blink phase where it is not shown.
    pub cursor_hidden: bool,
    /// How far the cursor shows, from 0 to 1, while it fades in and out.
    pub cursor_fade: f64,
    /// The links that get numbers, as the view finds them.
    pub hints: &'a [(String, i32, i32)],
    /// The matches of the search to highlight.
    pub matches: &'a [Match],
}

/// Background of search matches. Yellow reads well on every color scheme.
const SEARCH_HIGHLIGHT: Rgb = Rgb {
    r: 0xfd,
    g: 0xbc,
    b: 0x4b,
};
const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
const WHITE: Rgb = Rgb { r: 255, g: 255, b: 255 };

/// `alpha` of `over` on `under`, with 255 meaning only `over`.
#[must_use]
pub fn blend(over: Rgb, under: Rgb, alpha: u32) -> Rgb {
    let alpha = alpha.min(255);
    let mix = |top: u8, bottom: u8| ((u32::from(top) * alpha + u32::from(bottom) * (255 - alpha)) / 255) as u8;
    Rgb {
        r: mix(over.r, under.r),
        g: mix(over.g, under.g),
        b: mix(over.b, under.b),
    }
}

fn is_emoji(character: char) -> bool {
    matches!(u32::from(character), 0x2600..=0x27bf | 0x1f000..=0x1faff)
}

fn font_style(flags: Flags, character: char) -> FontStyle {
    FontStyle {
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        emoji: is_emoji(character),
    }
}

/// Selected text in the theme's colors for selections, as everywhere else on the desktop;
/// with `swapped`, text and background trade places instead.
fn selection_colors(foreground: Rgb, back: Rgb, scheme: &ColorScheme, swapped: bool) -> (Rgb, Rgb) {
    if swapped {
        (back, foreground)
    } else {
        (scheme.selection_text, scheme.selection_background)
    }
}

/// The rectangles of plain, double, curly, dotted or dashed underlines under `cells` cells.
fn underline_rects(geometry: &Geometry, column: i32, line: i32, cells: f64, flags: Flags) -> Vec<Rect> {
    let x = geometry.origin_x + f64::from(column) * geometry.cell_width;
    let y = geometry.origin_y + f64::from(line) * geometry.cell_height + geometry.ascent + 1.0;
    let width = cells * geometry.cell_width;
    let steps = width as i32;
    let line_at = |y: f64| Rect {
        x,
        y,
        width,
        height: 1.0,
    };
    let pixel = |step: i32, lift: f64| Rect {
        x: x + f64::from(step),
        y: y + lift,
        width: 1.0,
        height: 1.0,
    };
    let dots = |period: i32, on: i32| (0..steps).filter(move |step| step % period < on);
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        vec![line_at(y), line_at(y + 2.0)]
    } else if flags.contains(Flags::UNDERCURL) {
        (0..steps).map(|step| pixel(step, f64::from((step / 2) % 2))).collect()
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        dots(2, 1).map(|step| pixel(step, 0.0)).collect()
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        dots(5, 3).map(|step| pixel(step, 0.0)).collect()
    } else {
        vec![line_at(y)]
    }
}

/// The rectangles a cursor of this shape fills.
fn cursor_rects(geometry: &Geometry, column: i32, line: i32, shape: CursorShape) -> Vec<Rect> {
    let x = geometry.origin_x + f64::from(column) * geometry.cell_width;
    let y = geometry.origin_y + f64::from(line) * geometry.cell_height;
    let (w, h) = (geometry.cell_width, geometry.cell_height);
    let rect = |x, y, width, height| Rect { x, y, width, height };
    match shape {
        CursorShape::Block => vec![rect(x, y, w, h)],
        CursorShape::Underline => vec![rect(x, y + h - 2.0, w, 2.0)],
        CursorShape::Beam => vec![rect(x, y, 2.0, h)],
        // The hollow block that alacritty_terminal asks for when the terminal has no focus:
        // a frame of one pixel around the cell.
        _ => vec![
            rect(x, y, w, 1.0),
            rect(x, y + h - 1.0, w, 1.0),
            rect(x, y, 1.0, h),
            rect(x + w - 1.0, y, 1.0, h),
        ],
    }
}

/// Everything to paint over the background of the terminal: cells with their backgrounds,
/// underlines, strikeouts, box drawing and glyphs, then the cursor, the numbers of links, the
/// vertical line and the line numbers.
#[must_use]
pub fn frame<T: EventListener>(term: &Term<T>, options: &Options, scene: &Scene) -> Vec<Paint> {
    let scheme = scene.scheme;
    let geometry = &scene.geometry;
    let (grid_columns, grid_lines) = (term.columns() as i32, term.screen_lines() as i32);
    let line_base = term.grid().history_size() as i32 - term.grid().display_offset() as i32;
    let content = term.renderable_content();
    let palette = content.colors;
    let offset = content.display_offset as i32;
    let cursor = content.cursor;
    let selection = content.selection;
    let mut paints = Vec::new();
    let fill = |rect: Rect, color: Rgb, paints: &mut Vec<Paint>| paints.push(Paint::Fill(rect, color));

    let mut cursor_glyph = None;
    for cell in content.display_iter {
        let flags = cell.flags;
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let point = cell.point;
        let line = point.line.0 + offset;
        let column = point.column.0 as i32;
        let mut foreground = resolve(cell.fg, palette, scheme);
        let mut back = resolve(cell.bg, palette, scheme);
        if flags.contains(Flags::BOLD) && scene.bold_intense {
            foreground = brighten(cell.fg, foreground, palette, scheme);
        }
        if flags.contains(Flags::DIM) {
            foreground = faint(cell.fg, foreground, scheme);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut back);
        }
        if selection.is_some_and(|range| range.contains(point)) {
            (foreground, back) = selection_colors(foreground, back, scheme, options.invert_selection);
        } else if scene.matches.iter().any(|found| found.contains(&point)) {
            back = SEARCH_HIGHLIGHT;
            foreground = BLACK;
        }
        if flags.contains(Flags::HIDDEN) {
            foreground = back;
        }

        let cells = if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
        if back != scheme.background {
            fill(geometry.cell_rect(column, line, cells), back, &mut paints);
        }
        if flags.intersects(Flags::ALL_UNDERLINES) {
            let color = cell
                .underline_color()
                .map_or(foreground, |color| resolve(color, palette, scheme));
            for rect in underline_rects(geometry, column, line, cells, flags) {
                fill(rect, color, &mut paints);
            }
        }
        if flags.contains(Flags::STRIKEOUT) {
            let rect = Rect {
                x: geometry.origin_x + f64::from(column) * geometry.cell_width,
                y: f64::from(line) * geometry.cell_height + geometry.ascent * 0.65 + geometry.origin_y,
                width: cells * geometry.cell_width,
                height: 1.0,
            };
            fill(rect, foreground, &mut paints);
        }

        let drawn = linedraw::pieces(
            cell.c,
            geometry.cell_width,
            geometry.cell_height,
            options.own_line_characters,
            options.own_braille_characters,
        );
        if let Some(pieces) = drawn {
            let cell_rect = geometry.cell_rect(column, line, 1.0);
            for piece in pieces {
                let rect = Rect {
                    x: cell_rect.x + piece.x,
                    y: cell_rect.y + piece.y,
                    width: piece.width,
                    height: piece.height,
                };
                fill(rect, blend(foreground, back, piece.alpha), &mut paints);
            }
        } else if cell.c != ' ' && cell.c != '\0' {
            // Every glyph goes to its own cell: the font's advance is fractional, while the
            // cells sit on whole pixels so that columns line up.
            let mut text = String::from(cell.c);
            if let Some(zero_width) = cell.zerowidth() {
                text.extend(zero_width);
            }
            let (x, y) = geometry.baseline(column, line);
            paints.push(Paint::Text {
                x,
                y,
                text,
                color: foreground,
                font: font_style(flags, cell.c),
            });
        }
        if point == cursor.point {
            cursor_glyph = Some((cell.c, flags));
        }
    }

    if cursor.shape != CursorShape::Hidden && !scene.cursor_hidden {
        let line = cursor.point.line.0 + offset;
        let column = cursor.point.column.0 as i32;
        let shape = if scene.focused {
            cursor.shape
        } else {
            CursorShape::HollowBlock
        };
        let fade = (scene.cursor_fade.clamp(0.0, 1.0) * 255.0) as u32;
        let color = blend(
            options.cursor_color.unwrap_or(scheme.foreground),
            scheme.background,
            fade,
        );
        for rect in cursor_rects(geometry, column, line, shape) {
            fill(rect, color, &mut paints);
        }
        if shape == CursorShape::Block
            && let Some((glyph, flags)) = cursor_glyph.filter(|(glyph, _)| !glyph.is_whitespace())
        {
            let (x, y) = geometry.baseline(column, line);
            paints.push(Paint::Text {
                x,
                y,
                text: glyph.to_string(),
                color: options.cursor_text_color.unwrap_or(scheme.background),
                font: font_style(flags, glyph),
            });
        }
    }

    for (index, (_, line, column)) in crate::view::visible_hints(scene.hints, options.reverse_url_hints) {
        fill(geometry.cell_rect(*column, *line, 1.0), BLACK, &mut paints);
        let (x, y) = geometry.baseline(*column, *line);
        paints.push(Paint::Text {
            x,
            y,
            text: (index + 1).to_string(),
            color: WHITE,
            font: FontStyle::default(),
        });
    }

    if options.vertical_line > 0 {
        let rect = Rect {
            x: geometry.origin_x + f64::from(options.vertical_line) * geometry.cell_width,
            y: geometry.origin_y,
            width: 1.0,
            height: f64::from(grid_lines) * geometry.cell_height,
        };
        fill(rect, blend(scheme.foreground, scheme.background, 96), &mut paints);
    }
    if options.line_numbers {
        // At the end of each line, faded like faint text, so they stay out of the way.
        let color = colors::faded(colors::faded(scheme.foreground, scheme.background), scheme.background);
        for line in 0..grid_lines {
            let number = (line_base + line + 1).to_string();
            let column = (grid_columns - number.len() as i32).max(0);
            let (x, y) = geometry.baseline(column, line);
            paints.push(Paint::Text {
                x,
                y,
                text: number,
                color,
                font: FontStyle::default(),
            });
        }
    }
    paints
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line, Point, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};
    use alacritty_terminal::term::search::RegexSearch;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;

    const GEOMETRY: Geometry = Geometry {
        origin_x: 4.0,
        origin_y: 2.0,
        cell_width: 10.0,
        cell_height: 20.0,
        ascent: 15.0,
    };

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    /// A terminal of 10 columns and 3 lines that has received `output`.
    fn term_with(output: &str) -> Term<VoidListener> {
        let size = alacritty_terminal::term::test::TermSize::new(10, 3);
        let mut term = Term::new(alacritty_terminal::term::Config::default(), &size, VoidListener);
        Processor::<StdSyncHandler>::new().advance(&mut term, output.as_bytes());
        term
    }

    fn scene<'a>(scheme: &'a ColorScheme, hints: &'a [(String, i32, i32)], matches: &'a [Match]) -> Scene<'a> {
        Scene {
            geometry: GEOMETRY,
            scheme,
            bold_intense: true,
            focused: true,
            cursor_hidden: false,
            cursor_fade: 1.0,
            hints,
            matches,
        }
    }

    fn texts(paints: &[Paint]) -> Vec<(&str, Rgb, FontStyle)> {
        paints
            .iter()
            .filter_map(|paint| match paint {
                Paint::Text { text, color, font, .. } => Some((text.as_str(), *color, *font)),
                Paint::Fill(..) => None,
            })
            .collect()
    }

    fn fills(paints: &[Paint]) -> Vec<(Rect, Rgb)> {
        paints
            .iter()
            .filter_map(|paint| match paint {
                Paint::Fill(rect, color) => Some((*rect, *color)),
                Paint::Text { .. } => None,
            })
            .collect()
    }

    #[test]
    fn geometry_places_cells_on_the_grid() {
        assert_eq!(GEOMETRY.cell_rect(1, 2, 2.0), rect(14.0, 42.0, 20.0, 20.0));
        assert_eq!(GEOMETRY.baseline(1, 2), (14, 57));
    }

    #[test]
    fn blending_mixes_by_alpha() {
        let (red, blue) = (Rgb { r: 255, g: 0, b: 0 }, Rgb { r: 0, g: 0, b: 255 });
        assert_eq!(blend(red, blue, 255), red);
        assert_eq!(blend(red, blue, 0), blue);
        assert_eq!(blend(red, blue, 1000), red, "more than opaque is opaque");
        assert_eq!(blend(red, blue, 51), Rgb { r: 51, g: 0, b: 204 });
    }

    #[test]
    fn selected_text_stays_readable() {
        let scheme = colors::active();
        let (foreground, back) = selection_colors(scheme.foreground, scheme.background, &scheme, false);
        assert_eq!((foreground, back), (scheme.selection_text, scheme.selection_background));
        let inverted = selection_colors(scheme.foreground, scheme.background, &scheme, true);
        assert_eq!(inverted, (scheme.background, scheme.foreground));
    }

    #[test]
    fn underlines_have_their_own_patterns() {
        let plain = underline_rects(&GEOMETRY, 1, 1, 2.0, Flags::UNDERLINE);
        assert_eq!(plain, vec![rect(14.0, 38.0, 20.0, 1.0)]);
        let double = underline_rects(&GEOMETRY, 0, 0, 1.0, Flags::DOUBLE_UNDERLINE);
        assert_eq!(double, vec![rect(4.0, 18.0, 10.0, 1.0), rect(4.0, 20.0, 10.0, 1.0)]);
        let dotted = underline_rects(&GEOMETRY, 0, 0, 1.0, Flags::DOTTED_UNDERLINE);
        let columns: Vec<f64> = dotted.iter().map(|rect| rect.x - 4.0).collect();
        assert_eq!(columns, [0.0, 2.0, 4.0, 6.0, 8.0]);
        let dashed = underline_rects(&GEOMETRY, 0, 0, 1.0, Flags::DASHED_UNDERLINE);
        let columns: Vec<f64> = dashed.iter().map(|rect| rect.x - 4.0).collect();
        assert_eq!(columns, [0.0, 1.0, 2.0, 5.0, 6.0, 7.0]);
        let curl = underline_rects(&GEOMETRY, 0, 0, 1.0, Flags::UNDERCURL);
        let lifts: Vec<f64> = curl.iter().map(|rect| rect.y - 18.0).collect();
        assert_eq!(lifts, [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn cursors_fill_their_shape() {
        assert_eq!(
            cursor_rects(&GEOMETRY, 1, 0, CursorShape::Block),
            vec![rect(14.0, 2.0, 10.0, 20.0)]
        );
        assert_eq!(
            cursor_rects(&GEOMETRY, 1, 0, CursorShape::Underline),
            vec![rect(14.0, 20.0, 10.0, 2.0)]
        );
        assert_eq!(
            cursor_rects(&GEOMETRY, 1, 0, CursorShape::Beam),
            vec![rect(14.0, 2.0, 2.0, 20.0)]
        );
        let hollow = cursor_rects(&GEOMETRY, 1, 0, CursorShape::HollowBlock);
        assert_eq!(hollow.len(), 4);
        let thin = |length: f64| (length - 1.0).abs() < f64::EPSILON;
        assert!(hollow.iter().all(|rect| thin(rect.width) || thin(rect.height)));
    }

    #[test]
    fn cells_get_their_colors_and_fonts() {
        let scheme = colors::active();
        let options = Options::default();
        let term = term_with("a\x1b[1mb\x1b[0;3mc\x1b[0;7md\x1b[0;8me\x1b[0;2mf\x1b[0m\u{1f600}");
        let paints = frame(&term, &options, &scene(&scheme, &[], &[]));
        let found = texts(&paints);
        let regular = FontStyle::default();
        let bold = FontStyle { bold: true, ..regular };
        let italic = FontStyle {
            italic: true,
            ..regular
        };
        assert_eq!(found.first(), Some(&("a", scheme.foreground, regular)));
        assert_eq!(found.get(1), Some(&("b", scheme.foreground_intense, bold)));
        assert_eq!(found.get(2), Some(&("c", scheme.foreground, italic)));
        // Inverse text swaps its colors, hidden text takes the background's.
        assert_eq!(found.get(3), Some(&("d", scheme.background, regular)));
        assert_eq!(found.get(4), Some(&("e", scheme.background, regular)));
        assert_eq!(
            found.get(5).map(|text| text.1),
            Some(faint(
                alacritty_terminal::vte::ansi::Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground),
                scheme.foreground,
                &scheme
            ))
        );
        assert!(found.iter().any(|(text, _, font)| *text == "\u{1f600}" && font.emoji));
        // Only the inverse cell has a background of its own, besides the cursor.
        let backgrounds = fills(&paints);
        assert!(backgrounds.contains(&(GEOMETRY.cell_rect(3, 0, 1.0), scheme.foreground)));
    }

    #[test]
    fn bold_can_keep_its_color() {
        let scheme = colors::active();
        let term = term_with("\x1b[1mb");
        let mut normal = scene(&scheme, &[], &[]);
        normal.bold_intense = false;
        let painted = frame(&term, &Options::default(), &normal);
        let found = texts(&painted).first().map(|text| text.1);
        assert_eq!(found, Some(scheme.foreground));
    }

    #[test]
    fn lines_get_underlined_and_struck_through() {
        let scheme = colors::active();
        let term = term_with("\x1b[4;9mu\x1b[0m\x1b[58;2;1;2;3m\x1b[4mv");
        let paints = fills(&frame(&term, &Options::default(), &scene(&scheme, &[], &[])));
        assert!(
            paints.contains(&(rect(4.0, 18.0, 10.0, 1.0), scheme.foreground)),
            "underline"
        );
        assert!(
            paints.contains(&(rect(4.0, 11.75, 10.0, 1.0), scheme.foreground)),
            "strikeout"
        );
        assert!(
            paints.contains(&(rect(14.0, 18.0, 10.0, 1.0), Rgb { r: 1, g: 2, b: 3 })),
            "colored underline"
        );
    }

    #[test]
    fn box_drawing_comes_in_pieces() {
        let scheme = colors::active();
        let term = term_with("\u{2500}");
        let own = Options {
            own_line_characters: true,
            ..Options::default()
        };
        let drawn = frame(&term, &own, &scene(&scheme, &[], &[]));
        assert!(texts(&drawn).is_empty());
        assert!(fills(&drawn).len() > 1);
        let font = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        assert_eq!(texts(&font).first().map(|text| text.0), Some("\u{2500}"));
    }

    #[test]
    fn selections_and_matches_stand_out() {
        let scheme = colors::active();
        let mut term = term_with("abc abc");
        let mut selection = Selection::new(SelectionType::Simple, Point::new(Line(0), Column(0)), Side::Left);
        selection.update(Point::new(Line(0), Column(0)), Side::Right);
        term.selection = Some(selection);
        let mut regex = RegexSearch::new("abc").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(9));
        let matches: Vec<Match> = alacritty_terminal::term::search::RegexIter::new(
            start,
            end,
            alacritty_terminal::index::Direction::Right,
            &term,
            &mut regex,
        )
        .collect();
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &matches));
        let found = texts(&paints);
        assert_eq!(found.first().map(|text| text.1), Some(scheme.selection_text));
        assert_eq!(found.get(1).map(|text| text.1), Some(BLACK));
        let backgrounds = fills(&paints);
        assert!(backgrounds.contains(&(GEOMETRY.cell_rect(0, 0, 1.0), scheme.selection_background)));
        assert!(backgrounds.contains(&(GEOMETRY.cell_rect(4, 0, 1.0), SEARCH_HIGHLIGHT)));
        let swapped = Options {
            invert_selection: true,
            ..Options::default()
        };
        let inverted = frame(&term, &swapped, &scene(&scheme, &[], &matches));
        assert_eq!(texts(&inverted).first().map(|text| text.1), Some(scheme.background));
    }

    #[test]
    fn the_cursor_follows_focus_and_blinking() {
        let scheme = colors::active();
        let term = term_with("ab\x1b[1D");
        let options = Options {
            cursor_text_color: Some(Rgb { r: 9, g: 9, b: 9 }),
            ..Options::default()
        };
        let focused = frame(&term, &options, &scene(&scheme, &[], &[]));
        assert!(fills(&focused).contains(&(GEOMETRY.cell_rect(1, 0, 1.0), scheme.foreground)));
        assert_eq!(
            texts(&focused).last().map(|text| (text.0, text.1)),
            Some(("b", Rgb { r: 9, g: 9, b: 9 }))
        );
        let mut away = scene(&scheme, &[], &[]);
        away.focused = false;
        assert_eq!(fills(&frame(&term, &options, &away)).len(), 4, "a hollow block");
        let mut blinking = scene(&scheme, &[], &[]);
        blinking.cursor_hidden = true;
        assert!(fills(&frame(&term, &options, &blinking)).is_empty());
        let mut fading = scene(&scheme, &[], &[]);
        fading.cursor_fade = 0.0;
        assert!(fills(&frame(&term, &options, &fading)).contains(&(GEOMETRY.cell_rect(1, 0, 1.0), scheme.background)));
    }

    #[test]
    fn numbers_and_lines_go_on_top() {
        let scheme = colors::active();
        let term = term_with("x");
        let hints = vec![("https://a.org".to_string(), 1, 2)];
        let options = Options {
            vertical_line: 5,
            line_numbers: true,
            ..Options::default()
        };
        let paints = frame(&term, &options, &scene(&scheme, &hints, &[]));
        assert!(fills(&paints).contains(&(GEOMETRY.cell_rect(2, 1, 1.0), BLACK)));
        assert!(texts(&paints).contains(&("1", WHITE, FontStyle::default())));
        assert!(fills(&paints).iter().any(|(rect, _)| *rect
            == super::Rect {
                x: 54.0,
                y: 2.0,
                width: 1.0,
                height: 60.0
            }));
        let numbers: Vec<&str> = texts(&paints)
            .iter()
            .map(|text| text.0)
            .filter(|text| text.len() == 1)
            .collect();
        assert!(numbers.contains(&"2") && numbers.contains(&"3"), "{numbers:?}");
    }
}
