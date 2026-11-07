//! Set-of-marks (ADR-0039 section 2, slice 3): the model's screenshot with each boxed element of the snapshot outlined
//! and labelled with its ref's number, so "7" in the picture is e7 in the text, and a click on it goes by the same
//! exact path as any ref. Only the model's own screenshot is marked; the evidence captures stay as the screen was.
use super::raster::{Raster, Rect};

/// The most marks drawn, as a snapshot lists at most this many elements.
pub const MAX_MARKS: usize = 300;
/// An outline's thickness in screen pixels.
const OUTLINE: u32 = 2;
/// The smallest a digit is in the image the model receives, in pixels high.
const DIGIT_PIXELS: u32 = 10;
/// The label colours, in turn, so neighbours differ; each is dark enough for white digits.
const COLOURS: [[u8; 3]; 4] = [[200, 30, 30], [30, 70, 200], [20, 120, 40], [150, 30, 150]];
/// White, for the digits.
const DIGIT: [u8; 3] = [255, 255, 255];
/// The digits 0 to 9 in a 3 × 5 font, one bit a cell from the top-left, row by row.
const FONT: [u16; 10] = [
    0b111_101_101_101_111,
    0b010_110_010_010_111,
    0b111_001_111_100_111,
    0b111_001_111_001_111,
    0b101_101_111_001_001,
    0b111_100_111_001_111,
    0b111_100_111_101_111,
    0b111_001_010_010_010,
    0b111_101_111_101_111,
    0b111_101_111_001_111,
];

/// One element to mark: its ref's number and its box on the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mark {
    /// The ref's number (7 for e7).
    pub number: usize,
    /// Its box in screen pixels.
    pub rect: Rect,
}

/// Where a mark was drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placed {
    /// The ref's number.
    pub number: usize,
    /// The outlined box, inside the screen.
    pub rect: Rect,
    /// The label's box, inside the screen.
    pub label: Rect,
    /// The colour of its outline and label.
    pub colour: [u8; 3],
}

/// Draws the marks on `raster`, sized so a digit is still [`DIGIT_PIXELS`] high once the image is scaled to fit
/// `max_side`; a mark whose box is wholly off the screen is left out, and at most [`MAX_MARKS`] are drawn.
pub fn draw(raster: &mut Raster, marks: &[Mark], max_side: u32) -> Vec<Placed> {
    let unit = unit(raster.width.max(raster.height), max_side);
    let mut placed: Vec<Placed> = Vec::new();
    for mark in marks.iter().take(MAX_MARKS) {
        let Some(rect) = mark.rect.within(raster.width, raster.height) else {
            continue;
        };
        let colour = COLOURS[placed.len() % COLOURS.len()];
        let size = label_size(mark.number, unit);
        let taken: Vec<Rect> = placed.iter().map(|done| done.label).collect();
        let label = place(rect, size, (raster.width, raster.height), &taken);
        placed.push(Placed {
            number: mark.number,
            rect,
            label,
            colour,
        });
    }
    // Outlines first, then labels, so no outline crosses a label.
    for mark in &placed {
        raster.outline(mark.rect, OUTLINE, mark.colour);
    }
    for mark in &placed {
        raster.fill(mark.label, mark.colour);
        digits(raster, mark.label, mark.number, unit);
    }
    placed
}

/// Screen pixels per font cell: enough for [`DIGIT_PIXELS`] after scaling the longest side to `max_side`, at least 2.
fn unit(longest: u32, max_side: u32) -> u32 {
    let shown = if max_side == 0 {
        longest
    } else {
        max_side.min(longest)
    };
    // A digit is 5 cells high: 5 × unit × shown / longest ≥ DIGIT_PIXELS.
    let needed =
        (u64::from(DIGIT_PIXELS) * u64::from(longest)).div_ceil(5 * u64::from(shown.max(1)));
    u32::try_from(needed).unwrap_or(u32::MAX).max(2)
}

/// A label's width and height: the digits 3 cells wide with 1 between, and a cell of padding all round.
fn label_size(number: usize, unit: u32) -> (u32, u32) {
    let count = u32::try_from(number.to_string().len()).unwrap_or(u32::MAX);
    ((count * 4 + 1) * unit, 7 * unit)
}

/// The label's place: just above the box's top-left corner, else inside it, else below it, else at its top-right,
/// each kept inside the screen; the first that touches no earlier label, or else the first moved down until clear.
fn place(rect: Rect, (width, height): (u32, u32), screen: (u32, u32), taken: &[Rect]) -> Rect {
    let w = i64::from(width);
    let h = i64::from(height);
    let candidates = [
        (i64::from(rect.x), i64::from(rect.y) - h),
        (i64::from(rect.x), i64::from(rect.y)),
        (i64::from(rect.x), rect.bottom()),
        (rect.right() - w, i64::from(rect.y) - h),
    ];
    let inside = |(x, y): (i64, i64)| -> Rect {
        let x = x.clamp(0, (i64::from(screen.0) - w).max(0));
        let y = y.clamp(0, (i64::from(screen.1) - h).max(0));
        Rect {
            x: i32::try_from(x).unwrap_or(0),
            y: i32::try_from(y).unwrap_or(0),
            width,
            height,
        }
    };
    let clear = |label: &Rect| !taken.iter().any(|other| other.overlaps(label));
    if let Some(found) = candidates.into_iter().map(inside).find(clear) {
        return found;
    }
    let first = inside(candidates[0]);
    let mut y = i64::from(first.y);
    while y + h <= i64::from(screen.1) {
        let moved = inside((i64::from(first.x), y));
        if clear(&moved) {
            return moved;
        }
        y += h;
    }
    first
}

/// The number's digits in white inside its label.
fn digits(raster: &mut Raster, label: Rect, number: usize, unit: u32) {
    let unit_step = i32::try_from(unit).unwrap_or(i32::MAX);
    for (index, digit) in number.to_string().bytes().enumerate() {
        let glyph = FONT[usize::from(digit - b'0')];
        let left = label.x + unit_step * (1 + 4 * i32::try_from(index).unwrap_or(0));
        for cell in 0..15 {
            if glyph & (1 << (14 - cell)) == 0 {
                continue;
            }
            let (column, row) = (cell % 3, cell / 3);
            if let Some(square) = Rect::new(
                left + unit_step * column,
                label.y + unit_step * (1 + row),
                unit,
                unit,
            ) {
                raster.fill(square, DIGIT);
            }
        }
    }
}
