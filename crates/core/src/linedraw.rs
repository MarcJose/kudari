// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Box drawing, block and Braille characters drawn on the cell grid. Glyphs from a font
//! leave gaps between lines, above all with extra line spacing.
//!
//! Which arms a line character has comes from its Unicode name, see `tools/box_drawing.py`.

/// A filled rectangle in pixels relative to the cell, with its strength of 0 to 255.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub alpha: u32,
}

fn piece(x: f64, y: f64, width: f64, height: f64) -> Piece {
    Piece {
        x,
        y,
        width: width.max(0.0),
        height: height.max(0.0),
        alpha: 255,
    }
}

// BEGIN tables written by tools/box_drawing.py
/// Arms of U+2500 to U+257F: up, right, down, left; 0 none, 1 light, 2 heavy, 3 double.
/// All zero for dashes, arcs and diagonals, which stay with the font.
#[rustfmt::skip]
const ARMS: [[u8; 4]; 0x80] = [
    [0, 1, 0, 1], // ─ light horizontal
    [0, 2, 0, 2], // ━ heavy horizontal
    [1, 0, 1, 0], // │ light vertical
    [2, 0, 2, 0], // ┃ heavy vertical
    [0, 0, 0, 0], // ┄ light triple dash horizontal
    [0, 0, 0, 0], // ┅ heavy triple dash horizontal
    [0, 0, 0, 0], // ┆ light triple dash vertical
    [0, 0, 0, 0], // ┇ heavy triple dash vertical
    [0, 0, 0, 0], // ┈ light quadruple dash horizontal
    [0, 0, 0, 0], // ┉ heavy quadruple dash horizontal
    [0, 0, 0, 0], // ┊ light quadruple dash vertical
    [0, 0, 0, 0], // ┋ heavy quadruple dash vertical
    [0, 1, 1, 0], // ┌ light down and right
    [0, 2, 1, 0], // ┍ down light and right heavy
    [0, 1, 2, 0], // ┎ down heavy and right light
    [0, 2, 2, 0], // ┏ heavy down and right
    [0, 0, 1, 1], // ┐ light down and left
    [0, 0, 1, 2], // ┑ down light and left heavy
    [0, 0, 2, 1], // ┒ down heavy and left light
    [0, 0, 2, 2], // ┓ heavy down and left
    [1, 1, 0, 0], // └ light up and right
    [1, 2, 0, 0], // ┕ up light and right heavy
    [2, 1, 0, 0], // ┖ up heavy and right light
    [2, 2, 0, 0], // ┗ heavy up and right
    [1, 0, 0, 1], // ┘ light up and left
    [1, 0, 0, 2], // ┙ up light and left heavy
    [2, 0, 0, 1], // ┚ up heavy and left light
    [2, 0, 0, 2], // ┛ heavy up and left
    [1, 1, 1, 0], // ├ light vertical and right
    [1, 2, 1, 0], // ┝ vertical light and right heavy
    [2, 1, 1, 0], // ┞ up heavy and right down light
    [1, 1, 2, 0], // ┟ down heavy and right up light
    [2, 1, 2, 0], // ┠ vertical heavy and right light
    [2, 2, 1, 0], // ┡ down light and right up heavy
    [1, 2, 2, 0], // ┢ up light and right down heavy
    [2, 2, 2, 0], // ┣ heavy vertical and right
    [1, 0, 1, 1], // ┤ light vertical and left
    [1, 0, 1, 2], // ┥ vertical light and left heavy
    [2, 0, 1, 1], // ┦ up heavy and left down light
    [1, 0, 2, 1], // ┧ down heavy and left up light
    [2, 0, 2, 1], // ┨ vertical heavy and left light
    [2, 0, 1, 2], // ┩ down light and left up heavy
    [1, 0, 2, 2], // ┪ up light and left down heavy
    [2, 0, 2, 2], // ┫ heavy vertical and left
    [0, 1, 1, 1], // ┬ light down and horizontal
    [0, 1, 1, 2], // ┭ left heavy and right down light
    [0, 2, 1, 1], // ┮ right heavy and left down light
    [0, 2, 1, 2], // ┯ down light and horizontal heavy
    [0, 1, 2, 1], // ┰ down heavy and horizontal light
    [0, 1, 2, 2], // ┱ right light and left down heavy
    [0, 2, 2, 1], // ┲ left light and right down heavy
    [0, 2, 2, 2], // ┳ heavy down and horizontal
    [1, 1, 0, 1], // ┴ light up and horizontal
    [1, 1, 0, 2], // ┵ left heavy and right up light
    [1, 2, 0, 1], // ┶ right heavy and left up light
    [1, 2, 0, 2], // ┷ up light and horizontal heavy
    [2, 1, 0, 1], // ┸ up heavy and horizontal light
    [2, 1, 0, 2], // ┹ right light and left up heavy
    [2, 2, 0, 1], // ┺ left light and right up heavy
    [2, 2, 0, 2], // ┻ heavy up and horizontal
    [1, 1, 1, 1], // ┼ light vertical and horizontal
    [1, 1, 1, 2], // ┽ left heavy and right vertical light
    [1, 2, 1, 1], // ┾ right heavy and left vertical light
    [1, 2, 1, 2], // ┿ vertical light and horizontal heavy
    [2, 1, 1, 1], // ╀ up heavy and down horizontal light
    [1, 1, 2, 1], // ╁ down heavy and up horizontal light
    [2, 1, 2, 1], // ╂ vertical heavy and horizontal light
    [2, 1, 1, 2], // ╃ left up heavy and right down light
    [2, 2, 1, 1], // ╄ right up heavy and left down light
    [1, 1, 2, 2], // ╅ left down heavy and right up light
    [1, 2, 2, 1], // ╆ right down heavy and left up light
    [2, 2, 1, 2], // ╇ down light and up horizontal heavy
    [1, 2, 2, 2], // ╈ up light and down horizontal heavy
    [2, 1, 2, 2], // ╉ right light and left vertical heavy
    [2, 2, 2, 1], // ╊ left light and right vertical heavy
    [2, 2, 2, 2], // ╋ heavy vertical and horizontal
    [0, 0, 0, 0], // ╌ light double dash horizontal
    [0, 0, 0, 0], // ╍ heavy double dash horizontal
    [0, 0, 0, 0], // ╎ light double dash vertical
    [0, 0, 0, 0], // ╏ heavy double dash vertical
    [0, 3, 0, 3], // ═ double horizontal
    [3, 0, 3, 0], // ║ double vertical
    [0, 3, 1, 0], // ╒ down single and right double
    [0, 1, 3, 0], // ╓ down double and right single
    [0, 3, 3, 0], // ╔ double down and right
    [0, 0, 1, 3], // ╕ down single and left double
    [0, 0, 3, 1], // ╖ down double and left single
    [0, 0, 3, 3], // ╗ double down and left
    [1, 3, 0, 0], // ╘ up single and right double
    [3, 1, 0, 0], // ╙ up double and right single
    [3, 3, 0, 0], // ╚ double up and right
    [1, 0, 0, 3], // ╛ up single and left double
    [3, 0, 0, 1], // ╜ up double and left single
    [3, 0, 0, 3], // ╝ double up and left
    [1, 3, 1, 0], // ╞ vertical single and right double
    [3, 1, 3, 0], // ╟ vertical double and right single
    [3, 3, 3, 0], // ╠ double vertical and right
    [1, 0, 1, 3], // ╡ vertical single and left double
    [3, 0, 3, 1], // ╢ vertical double and left single
    [3, 0, 3, 3], // ╣ double vertical and left
    [0, 3, 1, 3], // ╤ down single and horizontal double
    [0, 1, 3, 1], // ╥ down double and horizontal single
    [0, 3, 3, 3], // ╦ double down and horizontal
    [1, 3, 0, 3], // ╧ up single and horizontal double
    [3, 1, 0, 1], // ╨ up double and horizontal single
    [3, 3, 0, 3], // ╩ double up and horizontal
    [1, 3, 1, 3], // ╪ vertical single and horizontal double
    [3, 1, 3, 1], // ╫ vertical double and horizontal single
    [3, 3, 3, 3], // ╬ double vertical and horizontal
    [0, 0, 0, 0], // ╭ light arc down and right
    [0, 0, 0, 0], // ╮ light arc down and left
    [0, 0, 0, 0], // ╯ light arc up and left
    [0, 0, 0, 0], // ╰ light arc up and right
    [0, 0, 0, 0], // ╱ light diagonal upper right to lower left
    [0, 0, 0, 0], // ╲ light diagonal upper left to lower right
    [0, 0, 0, 0], // ╳ light diagonal cross
    [0, 0, 0, 1], // ╴ light left
    [1, 0, 0, 0], // ╵ light up
    [0, 1, 0, 0], // ╶ light right
    [0, 0, 1, 0], // ╷ light down
    [0, 0, 0, 2], // ╸ heavy left
    [2, 0, 0, 0], // ╹ heavy up
    [0, 2, 0, 0], // ╺ heavy right
    [0, 0, 2, 0], // ╻ heavy down
    [0, 2, 0, 1], // ╼ light left and heavy right
    [1, 0, 2, 0], // ╽ light up and heavy down
    [0, 1, 0, 2], // ╾ heavy left and light right
    [2, 0, 1, 0], // ╿ heavy up and light down
];

/// Quarters of U+2596 to U+259F: 1 upper left, 2 upper right, 4 lower left, 8 lower right.
#[rustfmt::skip]
const QUARTERS: [u8; 10] = [
    4, // ▖ quadrant lower left
    8, // ▗ quadrant lower right
    1, // ▘ quadrant upper left
    13, // ▙ quadrant upper left and lower left and lower right
    9, // ▚ quadrant upper left and lower right
    7, // ▛ quadrant upper left and upper right and lower left
    11, // ▜ quadrant upper left and upper right and lower right
    2, // ▝ quadrant upper right
    6, // ▞ quadrant upper right and lower left
    14, // ▟ quadrant upper right and lower left and lower right
];
// END tables written by tools/box_drawing.py

const DOUBLE: u8 = 3;

/// The arms of a line character, or `None` if the font should draw it.
fn weights(character: u32) -> Option<[u8; 4]> {
    let index = character.checked_sub(0x2500)? as usize;
    ARMS.get(index).copied().filter(|arms| arms.iter().any(|arm| *arm > 0))
}

/// A light line is a ninth of the smaller side of the cell, but at least a pixel.
fn light_width(width: f64, height: f64) -> f64 {
    (width.min(height) / 9.0).round().max(1.0)
}

/// Where the light line through the middle of `extent` pixels starts.
fn middle(extent: f64, light: f64) -> f64 {
    ((extent - light) / 2.0).floor()
}

/// The strokes of a line of `strength` across `extent` pixels: start and thickness. A double
/// line is two light strokes with a light line's width of space between them.
fn strokes(strength: u8, extent: f64, light: f64) -> Vec<(f64, f64)> {
    let center = middle(extent, light);
    match strength {
        1 => vec![(center, light)],
        2 => vec![(middle(extent, 2.0 * light), 2.0 * light)],
        DOUBLE => vec![(center - light, light), (center + light, light)],
        _ => Vec::new(),
    }
}

/// One axis of the cell for the arms that run along it.
struct Axis {
    /// Length of the cell along the arms.
    along: f64,
    /// Length of the cell across them.
    across: f64,
    /// Strength of the crossing arm on the near side (up or left) and on the far side.
    near: u8,
    far: u8,
}

/// The stretch of one stroke along the axis, from the cell edge to where it meets the
/// crossing arms. `forward` arms run to the far edge, the others to the near one. `side` is
/// the crossing arm the stroke faces: the near one for the first stroke of a double line.
fn stretch(axis: &Axis, light: f64, double: bool, side: Option<bool>, forward: bool) -> (f64, f64) {
    let center = middle(axis.along, light);
    let near = strokes(axis.near, axis.along, light);
    let far = strokes(axis.far, axis.along, light);
    let crossing: Vec<&(f64, f64)> = near.iter().chain(far.iter()).collect();
    let meet = if double {
        let (facing, other, facing_strokes, other_strokes) = if side == Some(true) {
            (axis.near, axis.far, &near, &far)
        } else {
            (axis.far, axis.near, &far, &near)
        };
        // Facing a double line, the stroke stops at its inner stroke and leaves the gap
        // open; with the double line behind it, it runs on to the outer corner.
        let edge = |strokes: &Vec<(f64, f64)>| {
            strokes
                .first()
                .map_or(center, |(start, size)| if forward { *start } else { start + size })
        };
        match (facing, other) {
            (DOUBLE, _) if forward => center + light,
            (DOUBLE, _) => center,
            (1 | 2, _) => edge(facing_strokes),
            (_, DOUBLE) if forward => center - light,
            (_, DOUBLE) => center + 2.0 * light,
            (_, 1 | 2) => edge(other_strokes),
            _ if forward => center - light,
            _ => center + 2.0 * light,
        }
    } else if forward {
        crossing
            .iter()
            .map(|(start, _)| *start)
            .reduce(f64::min)
            .unwrap_or(center)
    } else {
        crossing
            .iter()
            .map(|(start, size)| start + size)
            .reduce(f64::max)
            .unwrap_or(center + light)
    };
    if forward {
        (meet, axis.along - meet)
    } else {
        (0.0, meet)
    }
}

/// The pieces of the arms that run along one axis, in the axis' own coordinates.
fn arms_along(axis: &Axis, light: f64, backward: u8, forward: u8) -> Vec<(f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for (strength, is_forward) in [(backward, false), (forward, true)] {
        let double = strength == DOUBLE;
        for (index, (start, size)) in strokes(strength, axis.across, light).into_iter().enumerate() {
            let side = double.then_some(index == 0);
            let (from, length) = stretch(axis, light, double, side, is_forward);
            out.push((from, length, start, size));
        }
    }
    out
}

fn lines(arms: [u8; 4], width: f64, height: f64) -> Vec<Piece> {
    let [up, right, down, left] = arms;
    let light = light_width(width, height);
    let horizontal = Axis {
        along: width,
        across: height,
        near: up,
        far: down,
    };
    let vertical = Axis {
        along: height,
        across: width,
        near: left,
        far: right,
    };
    let rows = arms_along(&horizontal, light, left, right)
        .into_iter()
        .map(|(x, length, y, size)| piece(x, y, length, size));
    let columns = arms_along(&vertical, light, up, down)
        .into_iter()
        .map(|(y, length, x, size)| piece(x, y, size, length));
    rows.chain(columns).collect()
}

/// Shares of the cell: halves, eighths, quarters and the three shades.
fn blocks(character: u32, width: f64, height: f64) -> Option<Vec<Piece>> {
    let (half_width, half_height) = ((width / 2.0).round(), (height / 2.0).round());
    let eighths = |extent: f64, count: u32| (extent * f64::from(count) / 8.0).round();
    let block = match character {
        0x2580 => piece(0.0, 0.0, width, half_height),
        0x2581..=0x2588 => {
            let top = eighths(height, 0x2588 - character);
            piece(0.0, top, width, height - top)
        }
        0x2589..=0x258f => piece(0.0, 0.0, eighths(width, 0x2590 - character), height),
        0x2590 => piece(half_width, 0.0, width - half_width, height),
        0x2591..=0x2593 => Piece {
            alpha: 64 * (character - 0x2590),
            ..piece(0.0, 0.0, width, height)
        },
        0x2594 => piece(0.0, 0.0, width, eighths(height, 1)),
        0x2595 => {
            let left = eighths(width, 7);
            piece(left, 0.0, width - left, height)
        }
        0x2596..=0x259f => {
            let bits = QUARTERS.get(character.checked_sub(0x2596)? as usize).copied()?;
            let quarters = [
                (1, piece(0.0, 0.0, half_width, half_height)),
                (2, piece(half_width, 0.0, width - half_width, half_height)),
                (4, piece(0.0, half_height, half_width, height - half_height)),
                (
                    8,
                    piece(half_width, half_height, width - half_width, height - half_height),
                ),
            ];
            return Some(
                quarters
                    .into_iter()
                    .filter(|(bit, _)| bits & bit != 0)
                    .map(|(_, quarter)| quarter)
                    .collect(),
            );
        }
        _ => return None,
    };
    Some(vec![block])
}

/// Square dots on a grid of two columns and four rows.
fn braille(character: u32, width: f64, height: f64) -> Vec<Piece> {
    // Unicode numbers the dots down the left column, then down the right one; dots 7 and 8
    // were added below them later.
    const DOTS: [(f64, f64); 8] = [
        (0.0, 0.0),
        (0.0, 1.0),
        (0.0, 2.0),
        (1.0, 0.0),
        (1.0, 1.0),
        (1.0, 2.0),
        (0.0, 3.0),
        (1.0, 3.0),
    ];
    let (column_width, row_height) = (width / 2.0, height / 4.0);
    let size = (column_width.min(row_height) / 2.0).round().max(1.0);
    DOTS.iter()
        .enumerate()
        .filter(|(bit, _)| character & (1 << bit) != 0)
        .map(|(_, (column, row))| {
            let x = ((column + 0.5) * column_width - size / 2.0).round();
            let y = ((row + 0.5) * row_height - size / 2.0).round();
            piece(x, y, size, size)
        })
        .collect()
}

/// The rectangles of a character; `None` leaves it to the font.
#[must_use]
pub fn pieces(character: char, width: f64, height: f64, own_lines: bool, own_braille: bool) -> Option<Vec<Piece>> {
    let code = u32::from(character);
    match code {
        0x2800..=0x28ff if own_braille => Some(braille(code, width, height)),
        0x2500..=0x257f if own_lines => weights(code).map(|arms| lines(arms, width, height)),
        0x2580..=0x259f if own_lines => blocks(code, width, height),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(pieces: &[Piece], x: f64, y: f64) -> bool {
        pieces
            .iter()
            .any(|p| x >= p.x && x < p.x + p.width && y >= p.y && y < p.y + p.height)
    }

    #[test]
    fn lines_reach_the_cell_edges() {
        let cross = pieces('┼', 8.0, 16.0, true, false).unwrap();
        assert!(cross.iter().any(|p| p.y.abs() < 0.001));
        assert!(cross.iter().any(|p| (p.y + p.height - 16.0).abs() < 0.001));
        assert!(cross.iter().any(|p| p.x.abs() < 0.001));
        assert!(cross.iter().any(|p| (p.x + p.width - 8.0).abs() < 0.001));
    }

    #[test]
    fn corners_stay_in_their_quadrant() {
        let corner = pieces('┌', 8.0, 16.0, true, false).unwrap();
        assert!(covers(&corner, 7.5, 7.5));
        assert!(covers(&corner, 3.5, 15.5));
        assert!(!covers(&corner, 0.5, 7.5));
        assert!(!covers(&corner, 3.5, 0.5));
    }

    #[test]
    fn inner_double_corners_stay_open() {
        let corner = pieces('╔', 9.0, 18.0, true, false).unwrap();
        let (x, y) = (((9.0_f64 - 1.0) / 2.0).floor(), ((18.0_f64 - 1.0) / 2.0).floor());
        let (near_x, far_x, near_y, far_y) = (x - 1.0, x + 1.0, y - 1.0, y + 1.0);
        assert!(covers(&corner, near_x + 0.5, near_y + 0.5), "outer corner");
        assert!(covers(&corner, far_x + 0.5, far_y + 0.5), "inner corner");
        assert!(!covers(&corner, far_x + 0.5, y + 0.5), "gap between the strokes");
        assert!(
            !covers(&corner, x + 0.5, far_y + 0.5),
            "the inner stroke starts at the inner corner"
        );
    }

    #[test]
    fn a_double_cross_leaves_all_four_gaps_open() {
        let cross = pieces('╬', 9.0, 18.0, true, false).unwrap();
        let (x, y) = (4.5, 8.5);
        assert!(!covers(&cross, x, y), "center");
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            assert!(!covers(&cross, x + dx, y + dy), "gap at {dx},{dy}");
        }
        for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            assert!(covers(&cross, x + dx, y + dy), "corner at {dx},{dy}");
        }
    }

    #[test]
    fn a_double_tee_keeps_its_straight_side_closed() {
        let tee = pieces('╦', 9.0, 18.0, true, false).unwrap();
        for x in 0..9 {
            assert!(covers(&tee, f64::from(x) + 0.5, 7.5), "top line at {x}");
        }
        assert!(!covers(&tee, 4.5, 9.5), "gap where the double line goes down");
    }

    #[test]
    fn heavy_lines_are_thicker_than_light_ones() {
        let light = pieces('─', 9.0, 18.0, true, false).unwrap();
        let heavy = pieces('━', 9.0, 18.0, true, false).unwrap();
        let thickness = |pieces: &[Piece]| pieces.iter().map(|p| p.height).fold(0.0, f64::max);
        assert!(thickness(&heavy) > thickness(&light));
    }

    #[test]
    fn half_lines_end_in_the_middle() {
        let half = pieces('╴', 8.0, 16.0, true, false).unwrap();
        assert!(covers(&half, 0.5, 7.5));
        assert!(!covers(&half, 7.5, 7.5));
    }

    #[test]
    fn all_line_tables_have_valid_entries() {
        for code in 0x2500..=0x257f {
            if let Some(arms) = weights(code) {
                assert!(
                    arms.iter().all(|arm| *arm <= 3) && arms.iter().any(|arm| *arm > 0),
                    "{code:x}"
                );
            }
        }
    }

    #[test]
    fn blocks_fill_their_share() {
        assert_eq!(
            pieces('█', 8.0, 16.0, true, false).unwrap(),
            vec![piece(0.0, 0.0, 8.0, 16.0)]
        );
        assert_eq!(
            pieces('▄', 8.0, 16.0, true, false).unwrap(),
            vec![piece(0.0, 8.0, 8.0, 8.0)]
        );
        assert_eq!(
            pieces('▌', 8.0, 16.0, true, false).unwrap(),
            vec![piece(0.0, 0.0, 4.0, 16.0)]
        );
        assert_eq!(pieces('▒', 8.0, 16.0, true, false).unwrap()[0].alpha, 128);
    }

    #[test]
    fn quadrants_follow_their_names() {
        let diagonal = pieces('▚', 8.0, 16.0, true, false).unwrap();
        assert!(covers(&diagonal, 1.0, 1.0));
        assert!(covers(&diagonal, 7.0, 15.0));
        assert!(!covers(&diagonal, 7.0, 1.0));
        assert!(!covers(&diagonal, 1.0, 15.0));
    }

    #[test]
    fn braille_dots_follow_the_bits() {
        assert_eq!(braille(0x2800, 8.0, 16.0).len(), 0);
        assert_eq!(braille(0x28ff, 8.0, 16.0).len(), 8);
        assert_eq!(pieces('⠁', 8.0, 16.0, false, false), None);
        let dot_four = braille(0x2808, 8.0, 16.0);
        assert!(dot_four.iter().all(|p| p.x >= 4.0 && p.y < 4.0), "dot 4 is top right");
    }

    #[test]
    fn every_character_has_valid_pieces_in_any_cell() {
        for (width, height) in [(1.0, 1.0), (3.0, 5.0), (8.0, 16.0), (9.0, 22.0), (40.0, 90.0)] {
            for code in 0x2500..=0x28ff {
                let Some(character) = char::from_u32(code) else {
                    continue;
                };
                for piece in pieces(character, width, height, true, true).unwrap_or_default() {
                    assert!(
                        piece.width >= 0.0 && piece.height >= 0.0,
                        "{code:x} in {width}x{height}: {piece:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn other_characters_stay_with_the_font() {
        assert_eq!(pieces('a', 8.0, 16.0, true, true), None);
        assert_eq!(pieces('╭', 8.0, 16.0, true, true), None);
        assert_eq!(pieces('┄', 8.0, 16.0, true, true), None);
    }
}
