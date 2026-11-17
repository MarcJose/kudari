// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What a terminal looks like, as a list of rectangles and texts in pixels. The core decides
//! colors, styles and shapes; a front end only paints the [`Paint`]s with its own fonts.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::search::Match;
use alacritty_terminal::term::{RenderableCursor, Term};
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
    /// Pixels of the device per logical pixel, as 1.25 at a scale of 125 %. The front end
    /// gives the origin, the cells and the ascent in whole pixels of the device.
    pub pixel_ratio: f64,
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

    /// `local`, in the coordinates of the cell at `column` and `line`, on whole pixels of the
    /// device, at least one thick, and then in those of the item. Each cell rounds the same
    /// numbers, as its corner is on a pixel already: a line is as thick in every cell, also
    /// where its edge falls on half a pixel, as a line of one logical pixel does at 150 %.
    #[must_use]
    pub fn within(&self, column: i32, line: i32, local: Rect) -> Rect {
        let ratio = if self.pixel_ratio > 0.0 { self.pixel_ratio } else { 1.0 };
        let snap = |start: f64, length: f64| {
            let from = (start * ratio).round();
            let to = ((start + length) * ratio).round().max(from + 1.0);
            (from / ratio, (to - from) / ratio)
        };
        let (x, width) = snap(local.x, local.width);
        let (y, height) = snap(local.y, local.height);
        let corner = self.cell_rect(column, line, 1.0);
        Rect {
            x: corner.x + x,
            y: corner.y + y,
            width,
            height,
        }
    }

    /// Where the text of a cell starts on its baseline. The front end gives the origin, the
    /// cells and the ascent in whole pixels of the device, so this lands on one of them as
    /// well: a fractional scale, as 125 %, would otherwise shift the text of some lines by a
    /// pixel against that of others.
    #[must_use]
    pub fn baseline(&self, column: i32, line: i32) -> (f64, f64) {
        (
            self.origin_x + f64::from(column) * self.cell_width,
            self.origin_y + f64::from(line) * self.cell_height + self.ascent,
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
    /// A fill that lets through what is under it, from 0 for nothing to 255 for opaque: the
    /// shades of ░▒▓ and a fading cursor, over the cell and, in a translucent terminal, the
    /// desktop.
    Shade(Rect, Rgb, u8),
    /// Text starting at a point of its baseline.
    Text {
        x: f64,
        y: f64,
        text: Glyph,
        color: Rgb,
        font: FontStyle,
    },
}

/// The text of a [`Paint::Text`]: nearly always the one character of a cell, which it keeps
/// without an allocation of its own, as a frame has one for each cell; a string only for a
/// cell with combining marks and for the few labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// One character, in UTF-8, and how many of the bytes it takes.
    Char([u8; 4], u8),
    Text(String),
}

impl Glyph {
    /// The character of a cell with its combining marks, as many as are drawn.
    fn of_cell(character: char, zero_width: Option<&[char]>) -> Self {
        match zero_width {
            Some(marks) if !marks.is_empty() => {
                let mut text = String::from(character);
                // A program can pile up any number on one cell, which every frame would shape.
                text.extend(marks.iter().take(MAX_ZERO_WIDTH));
                Self::Text(text)
            }
            _ => Self::from(character),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Char(bytes, length) => bytes
                .get(..usize::from(*length))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .unwrap_or_default(),
            Self::Text(text) => text,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }
}

impl From<char> for Glyph {
    fn from(character: char) -> Self {
        let mut bytes = [0; 4];
        let length = character.encode_utf8(&mut bytes).len() as u8;
        Self::Char(bytes, length)
    }
}

impl From<String> for Glyph {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
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
/// The combining characters drawn on one cell. Accents, variation selectors and the joined
/// emoji take a few; more only make every frame slower.
const MAX_ZERO_WIDTH: usize = 16;
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

/// The font of a character. Emoji take the emoji font where they show as pictures: wide ones,
/// and narrow ones with the variation selector for emoji. Narrow ones such as ✔ or ❤ are text
/// by default, and a picture would reach into the next cell.
fn font_style(flags: Flags, character: char, picture: bool) -> FontStyle {
    FontStyle {
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        emoji: picture && is_emoji(character),
    }
}

/// Whether a cell shows its character as a picture; see [`font_style`].
fn is_picture(flags: Flags, zero_width: Option<&[char]>) -> bool {
    flags.contains(Flags::WIDE_CHAR) || zero_width.is_some_and(|zero_width| zero_width.contains(&'\u{fe0f}'))
}

/// The text and background colors of a cell as the terminal shows it, before the selection
/// and the search: bold in the intense colors with `bold_intense`, faint, and swapped when
/// inverse. Hidden text takes its background afterwards.
#[must_use]
pub fn cell_colors(cell: &Cell, palette: &Colors, scheme: &ColorScheme, bold_intense: bool) -> (Rgb, Rgb) {
    let mut foreground = resolve(cell.fg, palette, scheme);
    let mut back = resolve(cell.bg, palette, scheme);
    if cell.flags.contains(Flags::BOLD) && bold_intense {
        foreground = brighten(cell.fg, foreground, palette, scheme);
    }
    if cell.flags.contains(Flags::DIM) {
        foreground = faint(cell.fg, foreground, palette, scheme);
    }
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut back);
    }
    (foreground, back)
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

/// The rectangles of plain, double, curly, dotted or dashed underlines under `cells` cells,
/// a pixel below the baseline, or higher where the font leaves less room below it: the lines
/// stay in their cell, which the next line would paint over.
fn underline_rects(geometry: &Geometry, column: i32, line: i32, cells: f64, flags: Flags) -> Vec<Rect> {
    let height = geometry.cell_height;
    // How many pixels the lines of each kind take below their first.
    let below = if flags.contains(Flags::DOUBLE_UNDERLINE) {
        2.0
    } else if flags.contains(Flags::UNDERCURL) {
        1.0
    } else {
        0.0
    };
    let y = (geometry.ascent + 1.0).min(height - 1.0 - below).max(0.0);
    let width = cells * geometry.cell_width;
    let steps = width as i32;
    let line_at = |y: f64| {
        geometry.within(
            column,
            line,
            Rect {
                x: 0.0,
                y,
                width,
                height: 1.0,
            },
        )
    };
    let pixel = |step: i32, lift: f64| {
        geometry.within(
            column,
            line,
            Rect {
                x: f64::from(step),
                y: y + lift,
                width: 1.0,
                height: 1.0,
            },
        )
    };
    let dots = |period: i32, on: i32| (0..steps).filter(move |step| step % period < on);
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        if height < 3.0 {
            vec![line_at(y)]
        } else {
            vec![line_at(y), line_at(y + 2.0)]
        }
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
/// The shapes of the cursor over `cells` cells: two on a wide character, so that its glyph
/// shows whole.
/// Whether the frame shows the cursor at all: the program has not hidden it (in vi mode it is
/// always there) and the view is not scrolled back so far that it is below the last line.
/// Only then can its blinking or fading change a frame.
pub fn cursor_on_screen<T: EventListener>(term: &Term<T>) -> bool {
    let content = term.renderable_content();
    shows(&content.cursor, content.display_offset, term.screen_lines())
}

fn shows(cursor: &RenderableCursor, offset: usize, lines: usize) -> bool {
    let line = i64::from(cursor.point.line.0) + offset as i64;
    cursor.shape != CursorShape::Hidden && (0..lines as i64).contains(&line)
}

fn cursor_rects(geometry: &Geometry, column: i32, line: i32, cells: f64, shape: CursorShape) -> Vec<Rect> {
    let (x, y) = (0.0, 0.0);
    let (w, h) = (cells * geometry.cell_width, geometry.cell_height);
    let rect = |x, y, width, height| geometry.within(column, line, Rect { x, y, width, height });
    match shape {
        CursorShape::Block => vec![rect(x, y, w, h)],
        CursorShape::Underline => vec![rect(x, y + h - 2.0, w, 2.0)],
        CursorShape::Beam => vec![rect(x, y, 2.0, h)],
        // The hollow block that `alacritty_terminal` asks for when the terminal has no focus:
        // a frame of one pixel around the cell.
        _ => vec![
            rect(x, y, w, 1.0),
            rect(x, y + h - 1.0, w, 1.0),
            rect(x, y, 1.0, h),
            rect(x + w - 1.0, y, 1.0, h),
        ],
    }
}

/// Cells next to each other in one line with the same background, from column `start` up to
/// before `end`.
#[derive(Clone, Copy)]
struct Run {
    line: i32,
    start: i32,
    end: i32,
    color: Rgb,
}

impl Run {
    fn fill(self, geometry: &Geometry, paints: &mut Vec<Paint>) {
        let rect = geometry.cell_rect(self.start, self.line, f64::from(self.end - self.start));
        paints.push(Paint::Fill(rect, self.color));
    }
}

/// Everything to paint over the background of the terminal: the backgrounds, underlines,
/// strikeouts and box drawing of the cells, then their glyphs, then the cursor, the numbers of
/// links, the vertical line and the line numbers. The glyphs come after all backgrounds, so
/// that the painter can draw them together, and so that the neighbour's background does not cut
/// off a glyph that reaches into the next cell, such as an italic one.
#[must_use]
pub fn frame<T: EventListener>(term: &Term<T>, options: &Options, scene: &Scene) -> Vec<Paint> {
    let mut paints = Vec::new();
    frame_into(term, options, scene, &mut paints);
    paints
}

/// The same as [`frame`] into `out`, which it clears first: a caller that keeps `out` from one
/// frame to the next allocates it once, not again for every frame.
pub fn frame_into<T: EventListener>(term: &Term<T>, options: &Options, scene: &Scene, out: &mut Vec<Paint>) {
    out.clear();
    let scheme = scene.scheme;
    let geometry = &scene.geometry;
    let (grid_columns, grid_lines) = (term.columns() as i32, term.screen_lines() as i32);
    let line_base = term.grid().history_size() as i32 - term.grid().display_offset() as i32;
    let content = term.renderable_content();
    let palette = content.colors;
    let offset = content.display_offset as i32;
    let cursor = content.cursor;
    let selection = content.selection;
    // The underlines, strikeouts and lines of the cells; the backgrounds go to `out` first.
    let mut paints = Vec::new();
    let fill = |rect: Rect, color: Rgb, paints: &mut Vec<Paint>| paints.push(Paint::Fill(rect, color));
    // Neighbouring cells of one color in a line make one rectangle: a coloured bar of a
    // program such as `mc`, or a selection over the screen, is then a few fills a line instead
    // of one for each cell. The backgrounds come before the underlines and the lines of the
    // cells, which stay within their cells.
    let backgrounds = out;
    let mut run: Option<Run> = None;

    // About one glyph a cell: room for them at once, rather than growing again and again.
    let mut glyphs = Vec::with_capacity((grid_columns * grid_lines).max(0) as usize);
    let mut cursor_glyph = None;
    // The matches come in the order of the cells, so one index walks along with them; a
    // search over all matches for each cell took long with a frequent pattern.
    let mut next_match = 0;
    let mut in_match = |point: Point| {
        while scene.matches.get(next_match).is_some_and(|found| *found.end() < point) {
            next_match += 1;
        }
        scene
            .matches
            .get(next_match)
            .is_some_and(|found| found.contains(&point))
    };
    for cell in content.display_iter {
        let flags = cell.flags;
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let point = cell.point;
        let line = point.line.0 + offset;
        let column = point.column.0 as i32;
        let (mut foreground, mut back) = cell_colors(cell.cell, palette, scheme, scene.bold_intense);
        if selection.is_some_and(|range| range.contains(point)) {
            (foreground, back) = selection_colors(foreground, back, scheme, options.invert_selection);
        } else if in_match(point) {
            back = SEARCH_HIGHLIGHT;
            foreground = BLACK;
        }
        if flags.contains(Flags::HIDDEN) {
            foreground = back;
        }

        let cells = if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
        let continues = run.is_some_and(|open| open.line == line && open.end == column && open.color == back);
        if continues {
            if let Some(open) = run.as_mut() {
                open.end += cells as i32;
            }
        } else {
            if let Some(done) = run.take() {
                done.fill(geometry, backgrounds);
            }
            if back != scheme.background {
                run = Some(Run {
                    line,
                    start: column,
                    end: column + cells as i32,
                    color: back,
                });
            }
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
            let local = Rect {
                x: 0.0,
                y: geometry.ascent * 0.65,
                width: cells * geometry.cell_width,
                height: 1.0,
            };
            let rect = geometry.within(column, line, local);
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
            for piece in pieces {
                let local = Rect {
                    x: piece.x,
                    y: piece.y,
                    width: piece.width,
                    height: piece.height,
                };
                let rect = geometry.within(column, line, local);
                if piece.alpha >= 255 {
                    fill(rect, foreground, &mut paints);
                } else {
                    paints.push(Paint::Shade(rect, foreground, piece.alpha as u8));
                }
            }
        } else if cell.c != ' ' && cell.c != '\0' {
            // Every glyph goes to its own cell: the font's advance is fractional, while the
            // cells sit on whole pixels so that columns line up.
            let text = Glyph::of_cell(cell.c, cell.zerowidth());
            let (x, y) = geometry.baseline(column, line);
            glyphs.push(Paint::Text {
                x,
                y,
                text,
                color: foreground,
                font: font_style(flags, cell.c, is_picture(flags, cell.zerowidth())),
            });
        }
        if point == cursor.point {
            // The whole cell, with its accents, which the block cursor covers; nothing of a
            // hidden one, as a program shows a password with SGR 8.
            let text = if flags.contains(Flags::HIDDEN) {
                Glyph::Text(String::new())
            } else {
                Glyph::of_cell(cell.c, cell.zerowidth())
            };
            cursor_glyph = Some((text, cell.c, flags, is_picture(flags, cell.zerowidth()), foreground));
        }
    }
    if let Some(done) = run.take() {
        done.fill(geometry, backgrounds);
    }
    backgrounds.append(&mut paints);
    let paints = backgrounds;
    paints.append(&mut glyphs);

    // While the view is scrolled back, the cursor can be below the last line: it is not on
    // screen then, and the space below the grid shows nothing of it.
    let cursor_line = cursor.point.line.0 + offset;
    // A cursor that has faded out entirely leaves the cell as it is.
    let fade = (scene.cursor_fade.clamp(0.0, 1.0) * 255.0) as u8;
    if shows(&cursor, content.display_offset, term.screen_lines()) && !scene.cursor_hidden && fade > 0 {
        let line = cursor_line;
        let column = cursor.point.column.0 as i32;
        let shape = if scene.focused {
            cursor.shape
        } else {
            CursorShape::HollowBlock
        };
        let color = options.cursor_color.unwrap_or(scheme.foreground);
        let cells = match cursor_glyph {
            Some((_, _, flags, ..)) if flags.contains(Flags::WIDE_CHAR) => 2.0,
            _ => 1.0,
        };
        for rect in cursor_rects(geometry, column, line, cells, shape) {
            if fade == 255 {
                fill(rect, color, paints);
            } else {
                paints.push(Paint::Shade(rect, color, fade));
            }
        }
        if shape == CursorShape::Block
            && let Some((text, glyph, flags, picture, foreground)) =
                cursor_glyph.filter(|(text, glyph, ..)| !text.is_empty() && !glyph.is_whitespace())
        {
            let (x, y) = geometry.baseline(column, line);
            paints.push(Paint::Text {
                x,
                y,
                text,
                // From the look of the cursor to the look of the cell as the cursor fades.
                color: blend(
                    options.cursor_text_color.unwrap_or(scheme.background),
                    foreground,
                    u32::from(fade),
                ),
                font: font_style(flags, glyph, picture),
            });
        }
    }

    for (index, (_, line, column)) in crate::view::visible_hints(scene.hints, options.reverse_url_hints) {
        fill(geometry.cell_rect(*column, *line, 1.0), BLACK, paints);
        let (x, y) = geometry.baseline(*column, *line);
        paints.push(Paint::Text {
            x,
            y,
            text: Glyph::Text((index + 1).to_string()),
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
        fill(rect, blend(scheme.foreground, scheme.background, 96), paints);
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
                text: Glyph::Text(number),
                color,
                font: FontStyle::default(),
            });
        }
    }
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
        pixel_ratio: 1.0,
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
                Paint::Fill(..) | Paint::Shade(..) => None,
            })
            .collect()
    }

    fn fills(paints: &[Paint]) -> Vec<(Rect, Rgb)> {
        paints
            .iter()
            .filter_map(|paint| match paint {
                Paint::Fill(rect, color) => Some((*rect, *color)),
                Paint::Text { .. } | Paint::Shade(..) => None,
            })
            .collect()
    }

    /// The text that the block cursor draws again over its cell.
    fn cursor_text(output: &str, options: &Options) -> Option<String> {
        let scheme = crate::colors::active();
        let color = options.cursor_text_color.unwrap_or(scheme.background);
        let paints = frame(&term_with(output), options, &scene(&scheme, &[], &[]));
        paints.iter().rev().find_map(|paint| match paint {
            Paint::Text { text, color: drawn, .. } if *drawn == color => Some(text.as_str().to_string()),
            _ => None,
        })
    }

    #[test]
    fn a_glyph_keeps_one_character_of_any_length_and_its_marks() {
        for character in ['a', 'é', '中', '😀'] {
            let glyph = Glyph::from(character);
            assert!(matches!(glyph, Glyph::Char(..)), "{character}");
            assert_eq!(glyph.as_str(), character.to_string());
        }
        let marked = Glyph::of_cell('e', Some(&['\u{301}', '\u{302}']));
        assert_eq!(marked.as_str(), "e\u{301}\u{302}");
        let piled = Glyph::of_cell('e', Some(&['\u{301}'; 40]));
        assert_eq!(piled.as_str().chars().count(), 1 + MAX_ZERO_WIDTH);
        assert!(matches!(Glyph::of_cell('x', Some(&[])), Glyph::Char(..)));
    }

    #[test]
    fn shades_let_through_what_is_under_them() {
        let scheme = crate::colors::active();
        let options = Options {
            own_line_characters: true,
            ..Options::default()
        };
        // Without the cursor, which has the color of the text.
        let paints = frame(
            &term_with("\x1b[?25l\u{2591}\u{2588}"),
            &options,
            &scene(&scheme, &[], &[]),
        );
        let shades: Vec<(Rgb, u8)> = paints
            .iter()
            .filter_map(|paint| match paint {
                Paint::Shade(_, color, alpha) => Some((*color, *alpha)),
                _ => None,
            })
            .collect();
        assert!(!shades.is_empty());
        assert!(
            shades
                .iter()
                .all(|(color, alpha)| *color == scheme.foreground && *alpha < 255),
            "{shades:?}"
        );
        // No opaque fill in a color mixed with the background of the scheme: the full block
        // is the only fill, in the color of the text.
        assert!(fills(&paints).iter().all(|(_, color)| *color == scheme.foreground));
    }

    #[test]
    fn a_cursor_below_the_screen_is_not_drawn() {
        let scheme = crate::colors::active();
        let mut term = term_with("1\r\n2\r\n3\r\n4\r\n5");
        let below_grid = |term: &Term<VoidListener>| {
            fills(&frame(term, &Options::default(), &scene(&scheme, &[], &[])))
                .iter()
                .any(|(rect, _)| rect.y + rect.height > GEOMETRY.origin_y + 3.0 * GEOMETRY.cell_height)
        };
        assert!(!below_grid(&term));
        // One line back: the cursor in the last line is below the screen now.
        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(1));
        assert!(!below_grid(&term));
        let cursor = scheme.foreground;
        let drawn = fills(&frame(&term, &Options::default(), &scene(&scheme, &[], &[])));
        assert!(drawn.iter().all(|(_, color)| *color != cursor), "{drawn:?}");
    }

    #[test]
    fn the_cursor_is_on_screen_only_where_a_frame_draws_it() {
        let scheme = crate::colors::active();
        let drawn = |term: &Term<VoidListener>| {
            fills(&frame(term, &Options::default(), &scene(&scheme, &[], &[])))
                .iter()
                .any(|(_, color)| *color == scheme.foreground)
        };
        let mut term = term_with("1\r\n2\r\n3\r\n4\r\n5");
        assert!(cursor_on_screen(&term) && drawn(&term));
        // Scrolled back by one line: the cursor in the last line is below the screen.
        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(1));
        assert!(!cursor_on_screen(&term) && !drawn(&term));
        term.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
        assert!(cursor_on_screen(&term));
        // Hidden by the program (DECTCEM), as `htop` does, and shown again.
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, b"\x1b[?25l");
        assert!(!cursor_on_screen(&term) && !drawn(&term));
        // In vi mode, the cursor of vi mode shows even then.
        term.toggle_vi_mode();
        assert!(cursor_on_screen(&term) && drawn(&term));
        term.toggle_vi_mode();
        parser.advance(&mut term, b"\x1b[?25h");
        assert!(cursor_on_screen(&term) && drawn(&term));
    }

    #[test]
    fn hidden_text_stays_hidden_under_the_block_cursor() {
        // A PIN behind SGR 8, the cursor on its first digit; the text of the cursor in a color
        // of its own, so that it shows.
        let options = Options {
            cursor_text_color: Some(Rgb { r: 1, g: 2, b: 3 }),
            ..Options::default()
        };
        assert_eq!(cursor_text("\x1b[8m1234\x1b[0m\x1b[4D", &options), None);
        assert_eq!(cursor_text("1234\x1b[4D", &options).as_deref(), Some("1"));
    }

    #[test]
    fn the_block_cursor_draws_the_accents_of_its_cell() {
        // An e with a combining acute accent, the cursor on it.
        assert_eq!(
            cursor_text("e\u{301}x\x08\x08", &Options::default()).as_deref(),
            Some("e\u{301}")
        );
    }

    #[test]
    fn underlines_stay_in_their_cell_with_little_room_below_the_baseline() {
        // A small font: the baseline 3 pixels above the bottom of the cell.
        let tight = Geometry {
            cell_height: GEOMETRY.ascent + 3.0,
            ..GEOMETRY
        };
        let bottom = tight.origin_y + tight.cell_height;
        for flags in [
            Flags::UNDERLINE,
            Flags::DOUBLE_UNDERLINE,
            Flags::UNDERCURL,
            Flags::DOTTED_UNDERLINE,
            Flags::DASHED_UNDERLINE,
        ] {
            let rects = underline_rects(&tight, 0, 0, 1.0, flags);
            assert!(!rects.is_empty(), "{flags:?}");
            assert!(
                rects.iter().all(|rect| rect.y + rect.height <= bottom),
                "{flags:?}: {rects:?}"
            );
        }
        // With room enough, the lines stay a pixel below the baseline.
        let plain = underline_rects(&GEOMETRY, 0, 0, 1.0, Flags::DOUBLE_UNDERLINE);
        assert_eq!(plain[0].y, GEOMETRY.origin_y + GEOMETRY.ascent + 1.0);
    }

    #[test]
    fn only_emoji_that_show_as_pictures_take_the_emoji_font() {
        let scheme = crate::colors::active();
        // A check mark and a heart are text; the heart with the selector for emoji and the
        // wide face are pictures.
        let term = term_with("\u{2714}\u{2764} \u{2764}\u{fe0f}\u{1f600}");
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        let emoji: Vec<(&str, bool)> = texts(&paints)
            .iter()
            .map(|(text, _, font)| (*text, font.emoji))
            .collect();
        assert_eq!(
            emoji,
            [
                ("\u{2714}", false),
                ("\u{2764}", false),
                ("\u{2764}\u{fe0f}", true),
                ("\u{1f600}", true)
            ]
        );
    }

    #[test]
    fn bold_text_keeps_the_foreground_that_the_program_set() {
        let scheme = crate::colors::active();
        let term = term_with("\x1b]10;#00ff00\x07a\x1b[1mb");
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        let green = Rgb { r: 0, g: 255, b: 0 };
        let colors: Vec<_> = texts(&paints)
            .into_iter()
            .map(|(text, color, _)| (text, color))
            .collect();
        assert!(
            colors.contains(&("a", green)) && colors.contains(&("b", green)),
            "{colors:?}"
        );
    }

    #[test]
    fn the_block_cursor_covers_a_wide_character_whole() {
        // 你好 and the cursor moved back onto 好.
        let scheme = crate::colors::active();
        let term = term_with("\u{4f60}\u{597d}\x1b[2D");
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        let cursor = fills(&paints)
            .into_iter()
            .find(|(_, color)| *color == scheme.foreground)
            .map(|(rect, _)| rect);
        assert_eq!(cursor.map(|rect| rect.width), Some(2.0 * GEOMETRY.cell_width));
        assert_eq!(
            cursor_rects(&GEOMETRY, 2, 0, 2.0, CursorShape::Underline)[0].width,
            2.0 * GEOMETRY.cell_width
        );
    }

    #[test]
    fn a_pile_of_combining_characters_is_cut() {
        let scheme = crate::colors::active();
        let accents: String = std::iter::repeat_n('\u{301}', 1000).collect();
        let term = term_with(&format!("a{accents}"));
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        let drawn = texts(&paints)
            .into_iter()
            .find(|(text, _, _)| text.starts_with('a'))
            .unwrap()
            .0;
        assert_eq!(drawn.chars().count(), 1 + MAX_ZERO_WIDTH);
        // An ordinary accent stays as it is.
        let term = term_with("e\u{301}");
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &[]));
        assert!(texts(&paints).iter().any(|(text, _, _)| *text == "e\u{301}"));
    }

    #[test]
    fn many_matches_highlight_the_cells_they_cover() {
        // Every other cell matches, in the order a search gives them.
        let scheme = crate::colors::active();
        let term = term_with("ababababab\r\nbababababa");
        let matches: Vec<Match> = (0..2_usize)
            .flat_map(|line| (0..10_usize).map(move |column| (line, column)))
            .filter(|(line, column)| (line + column) % 2 == 0)
            .map(|(line, column)| {
                let point = Point::new(Line(line as i32), Column(column));
                point..=point
            })
            .collect();
        let paints = frame(&term, &Options::default(), &scene(&scheme, &[], &matches));
        let highlighted: Vec<String> = texts(&paints)
            .into_iter()
            .filter(|(_, color, _)| *color == BLACK)
            .map(|(text, _, _)| text.to_string())
            .collect();
        assert_eq!(highlighted.concat(), "aaaaaaaaaa");
        let backgrounds = fills(&paints)
            .into_iter()
            .filter(|(_, color)| *color == SEARCH_HIGHLIGHT)
            .count();
        assert_eq!(backgrounds, 10);
    }

    #[test]
    fn geometry_places_cells_on_the_grid() {
        assert_eq!(GEOMETRY.cell_rect(1, 2, 2.0), rect(14.0, 42.0, 20.0, 20.0));
        assert_eq!(GEOMETRY.baseline(1, 2), (14.0, 57.0));
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
            cursor_rects(&GEOMETRY, 1, 0, 1.0, CursorShape::Block),
            vec![rect(14.0, 2.0, 10.0, 20.0)]
        );
        assert_eq!(
            cursor_rects(&GEOMETRY, 1, 0, 1.0, CursorShape::Underline),
            vec![rect(14.0, 20.0, 10.0, 2.0)]
        );
        assert_eq!(
            cursor_rects(&GEOMETRY, 1, 0, 1.0, CursorShape::Beam),
            vec![rect(14.0, 2.0, 2.0, 20.0)]
        );
        let hollow = cursor_rects(&GEOMETRY, 1, 0, 1.0, CursorShape::HollowBlock);
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
                &alacritty_terminal::term::color::Colors::default(),
                &scheme
            ))
        );
        assert!(found.iter().any(|(text, _, font)| *text == "\u{1f600}" && font.emoji));
        // Only the inverse cell has a background of its own, besides the cursor.
        let backgrounds = fills(&paints);
        assert!(backgrounds.contains(&(GEOMETRY.cell_rect(3, 0, 1.0), scheme.foreground)));
    }

    #[test]
    fn neighbouring_cells_of_one_background_make_one_rectangle() {
        let scheme = colors::active();
        let red = Rgb { r: 200, g: 0, b: 0 };
        let blue = Rgb { r: 0, g: 0, b: 200 };
        // A red bar over four cells with a wide character in it, a gap, two red cells, then
        // a blue one right after them; on the next line, red again.
        let term = term_with(
            "\x1b[48;2;200;0;0mab\u{4e2d}\x1b[0m \x1b[48;2;200;0;0mcd\x1b[48;2;0;0;200me\x1b[0m\r\n\x1b[48;2;200;0;0mf\x1b[0m",
        );
        let mut quiet = scene(&scheme, &[], &[]);
        quiet.cursor_hidden = true;
        let backgrounds = fills(&frame(&term, &Options::default(), &quiet));
        assert_eq!(
            backgrounds,
            [
                (GEOMETRY.cell_rect(0, 0, 4.0), red),
                (GEOMETRY.cell_rect(5, 0, 2.0), red),
                (GEOMETRY.cell_rect(7, 0, 1.0), blue),
                (GEOMETRY.cell_rect(0, 1, 1.0), red),
            ]
        );
    }

    #[test]
    fn a_selection_over_the_screen_is_one_rectangle_a_line() {
        let scheme = colors::active();
        let mut term = term_with("abcdefghij\r\nklmnopqrst\r\nuvwxyz");
        let mut selection = Selection::new(SelectionType::Lines, Point::new(Line(0), Column(0)), Side::Left);
        selection.update(Point::new(Line(2), Column(9)), Side::Right);
        term.selection = Some(selection);
        let mut quiet = scene(&scheme, &[], &[]);
        quiet.cursor_hidden = true;
        let backgrounds = fills(&frame(&term, &Options::default(), &quiet));
        assert_eq!(
            backgrounds,
            (0..3)
                .map(|line| (GEOMETRY.cell_rect(0, line, 10.0), scheme.selection_background))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn glyphs_come_after_the_backgrounds_of_all_cells() {
        let scheme = colors::active();
        // Colors of their own, which no palette makes the same.
        let term = term_with("\x1b[48;2;200;0;0ma\x1b[48;2;0;200;0mb\x1b[0m\r\n\x1b[48;2;0;0;200mc\x1b[0m");
        let mut quiet = scene(&scheme, &[], &[]);
        quiet.cursor_hidden = true;
        let paints = frame(&term, &Options::default(), &quiet);
        let kinds: Vec<&str> = paints
            .iter()
            .map(|paint| match paint {
                Paint::Fill(..) | Paint::Shade(..) => "fill",
                Paint::Text { text, .. } => text.as_str(),
            })
            .collect();
        assert_eq!(kinds, ["fill", "fill", "fill", "a", "b", "c"]);
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
    fn lines_are_as_thick_in_every_cell_at_a_fractional_scale() {
        // At 150 %, a line of one logical pixel is a pixel and a half of the device: in
        // cells of whole device pixels, it rounds the same way in every one of them.
        let ratio = 1.5;
        let geometry = Geometry {
            origin_x: 2.0 / ratio,
            origin_y: 2.0 / ratio,
            cell_width: 14.0 / ratio,
            cell_height: 26.0 / ratio,
            ascent: 20.0 / ratio,
            pixel_ratio: ratio,
        };
        let pieces = linedraw::pieces('┼', geometry.cell_width, geometry.cell_height, true, false).unwrap();
        let device = |value: f64| value * ratio;
        let mut thicknesses = std::collections::BTreeSet::new();
        for line in 0..40 {
            for column in 0..40 {
                for piece in &pieces {
                    let local = Rect {
                        x: piece.x,
                        y: piece.y,
                        width: piece.width,
                        height: piece.height,
                    };
                    let rect = geometry.within(column, line, local);
                    for edge in [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height] {
                        assert!((device(edge) - device(edge).round()).abs() < 1e-6, "{rect:?}");
                    }
                    thicknesses.insert((
                        device(rect.width.min(rect.height)).round() as i64,
                        piece.width < piece.height,
                    ));
                }
            }
        }
        // One thickness for the vertical arm and one for the horizontal.
        assert_eq!(thicknesses.len(), 2, "{thicknesses:?}");
        // A line of less than a pixel of the device still shows.
        let thin = Geometry {
            pixel_ratio: 0.5,
            ..GEOMETRY
        };
        let rect = thin.within(0, 0, rect(0.0, 0.0, 1.0, 1.0));
        assert!((rect.height * 0.5 - 1.0).abs() < 1e-9);
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
            // At 65 % of the ascent, on the pixel that Qt filled for 11.75 as well.
            paints.contains(&(rect(4.0, 12.0, 10.0, 1.0), scheme.foreground)),
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
        // The three cells of the match make one rectangle.
        assert!(backgrounds.contains(&(GEOMETRY.cell_rect(4, 0, 3.0), SEARCH_HIGHLIGHT)));
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
        // A fading cursor lets through the cell and what is under the terminal, instead of
        // mixing with the background of the scheme.
        let mut fading = scene(&scheme, &[], &[]);
        fading.cursor_fade = 0.5;
        let paints = frame(&term, &options, &fading);
        assert!(paints.contains(&Paint::Shade(GEOMETRY.cell_rect(1, 0, 1.0), scheme.foreground, 127)));
        assert!(
            !fills(&paints)
                .iter()
                .any(|(rect, _)| *rect == GEOMETRY.cell_rect(1, 0, 1.0))
        );
        // Its character goes from the color of the cursor text to its own, and stays visible.
        let half = blend(Rgb { r: 9, g: 9, b: 9 }, scheme.foreground, 127);
        assert_eq!(texts(&paints).last().map(|text| (text.0, text.1)), Some(("b", half)));
        // Faded out, the cursor leaves the cell as it is.
        fading.cursor_fade = 0.0;
        let paints = frame(&term, &options, &fading);
        assert!(!paints.iter().any(|paint| matches!(paint, Paint::Shade(..))));
        assert_eq!(texts(&paints).iter().filter(|text| text.0 == "b").count(), 1);
        assert_eq!(texts(&paints).last().map(|text| text.1), Some(scheme.foreground));
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
