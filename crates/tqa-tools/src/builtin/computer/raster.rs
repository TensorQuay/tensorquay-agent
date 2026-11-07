//! The screen as pixels (ADR-0039 slice 3): Xvfb's screen file read as an RGB raster, drawn on, and scaled down for a
//! route that takes smaller images.

/// The XWD header's fixed fields: 25 big-endian 32-bit words.
const XWD_FIELDS: usize = 25;
/// The XWD format version Xvfb writes.
const XWD_VERSION: u32 = 7;
/// `ZPixmap`, the only layout Xvfb writes.
const Z_PIXMAP: u32 = 2;
/// The largest side read, as the store's screenshots allow.
const MAX_SIDE: u32 = tqa_types::MAX_SCREENSHOT_SIDE;

/// An RGB image, 3 bytes a pixel, row by row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Raster {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its pixels.
    pub rgb: Vec<u8>,
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    /// Its left edge.
    pub x: i32,
    /// Its top edge.
    pub y: i32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

impl Rect {
    /// The rectangle, or `None` when it has no area.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                x,
                y,
                width,
                height,
            })
        }
    }

    /// Its right edge, exclusive.
    #[must_use]
    pub fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    /// Its bottom edge, exclusive.
    #[must_use]
    pub fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    /// Whether it shares any pixel with `other`.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        i64::from(self.x) < other.right()
            && i64::from(other.x) < self.right()
            && i64::from(self.y) < other.bottom()
            && i64::from(other.y) < self.bottom()
    }

    /// Its part inside a `width × height` screen, or `None` when none of it is.
    #[must_use]
    pub fn within(&self, width: u32, height: u32) -> Option<Self> {
        let left = i64::from(self.x).max(0);
        let top = i64::from(self.y).max(0);
        let right = self.right().min(i64::from(width));
        let bottom = self.bottom().min(i64::from(height));
        if right <= left || bottom <= top {
            return None;
        }
        Self::new(
            i32::try_from(left).ok()?,
            i32::try_from(top).ok()?,
            u32::try_from(right - left).ok()?,
            u32::try_from(bottom - top).ok()?,
        )
    }

    /// Its centre.
    #[must_use]
    pub fn centre(&self) -> (i64, i64) {
        (
            i64::from(self.x) + i64::from(self.width / 2),
            i64::from(self.y) + i64::from(self.height / 2),
        )
    }
}

/// Why the screen file couldn't be read as an image.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the screen file isn't an image Xvfb writes: {0}")]
pub struct XwdError(&'static str);

impl Raster {
    /// A raster of one colour.
    #[must_use]
    pub fn filled(width: u32, height: u32, colour: [u8; 3]) -> Self {
        Self {
            width,
            height,
            rgb: colour.repeat(width as usize * height as usize),
        }
    }

    /// Xvfb's screen file (`-fbdir`): an XWD header, its colour map, then 24- or 32-bit `ZPixmap` pixels with
    /// true-colour masks, in the header's byte order.
    ///
    /// # Errors
    /// The part that isn't as Xvfb writes it.
    pub fn from_xwd(bytes: &[u8]) -> Result<Self, XwdError> {
        let field = |index: usize| -> Result<u32, XwdError> {
            let at = index * 4;
            bytes
                .get(at..at + 4)
                .and_then(|word| word.try_into().ok())
                .map(u32::from_be_bytes)
                .ok_or(XwdError("a short header"))
        };
        let header_size = field(0)? as usize;
        if header_size < XWD_FIELDS * 4 || field(1)? != XWD_VERSION || field(2)? != Z_PIXMAP {
            return Err(XwdError("not a version 7 ZPixmap"));
        }
        let (width, height) = (field(4)?, field(5)?);
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(XwdError("a size out of bounds"));
        }
        let little = field(7)? == 0;
        let bits = field(11)?;
        let line = field(12)? as usize;
        let masks = [field(14)?, field(15)?, field(16)?];
        let colours = field(19)? as usize;
        if !(bits == 32 || bits == 24) || masks.contains(&0) {
            return Err(XwdError("not 24- or 32-bit true colour"));
        }
        let step = bits as usize / 8;
        if line < width as usize * step {
            return Err(XwdError("a line shorter than its pixels"));
        }
        let start = header_size + colours * 12;
        let end = start + line * height as usize;
        let pixels = bytes.get(start..end).ok_or(XwdError("short pixels"))?;
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for row in pixels.chunks_exact(line) {
            for pixel in row[..width as usize * step].chunks_exact(step) {
                let value = word(pixel, little);
                rgb.extend(masks.map(|mask| channel(value, mask)));
            }
        }
        Ok(Self { width, height, rgb })
    }

    /// The colour at a pixel inside the raster.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let at = (y as usize * self.width as usize + x as usize) * 3;
        [self.rgb[at], self.rgb[at + 1], self.rgb[at + 2]]
    }

    /// Fills the part of `rect` inside the raster.
    pub fn fill(&mut self, rect: Rect, colour: [u8; 3]) {
        let Some(rect) = rect.within(self.width, self.height) else {
            return;
        };
        let (left, top) = (rect.x.unsigned_abs(), rect.y.unsigned_abs());
        for y in top..top + rect.height {
            let start = (y as usize * self.width as usize + left as usize) * 3;
            for pixel in self.rgb[start..start + rect.width as usize * 3].chunks_exact_mut(3) {
                pixel.copy_from_slice(&colour);
            }
        }
    }

    /// Draws `rect`'s outline, `thickness` pixels inwards, where it is inside the raster.
    pub fn outline(&mut self, rect: Rect, thickness: u32, colour: [u8; 3]) {
        let edge = thickness.min(rect.width).min(rect.height);
        let sides = [
            Rect::new(rect.x, rect.y, rect.width, edge),
            Rect::new(
                rect.x,
                i32::try_from(rect.bottom() - i64::from(edge)).unwrap_or(i32::MAX),
                rect.width,
                edge,
            ),
            Rect::new(rect.x, rect.y, edge, rect.height),
            Rect::new(
                i32::try_from(rect.right() - i64::from(edge)).unwrap_or(i32::MAX),
                rect.y,
                edge,
                rect.height,
            ),
        ];
        for side in sides.into_iter().flatten() {
            self.fill(side, colour);
        }
    }

    /// The raster scaled down to fit within `max_side` on both sides, each new pixel the average of the ones it covers;
    /// unchanged when it already fits.
    #[must_use]
    pub fn fitted(&self, max_side: u32) -> Self {
        let longest = self.width.max(self.height);
        if max_side == 0 || longest <= max_side {
            return self.clone();
        }
        let scale = |side: u32| {
            u32::try_from(u64::from(side) * u64::from(max_side) / u64::from(longest))
                .unwrap_or(1)
                .max(1)
        };
        let (width, height) = (scale(self.width), scale(self.height));
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            let (top, bottom) = span(y, height, self.height);
            for x in 0..width {
                let (left, right) = span(x, width, self.width);
                rgb.extend(self.average((left, right), (top, bottom)));
            }
        }
        Self { width, height, rgb }
    }

    /// The average colour of the pixels `[left, right) × [top, bottom)`.
    fn average(&self, (left, right): (u32, u32), (top, bottom): (u32, u32)) -> [u8; 3] {
        let mut sum = [0_u64; 3];
        let pixels = (top..bottom).flat_map(|y| (left..right).map(move |x| (x, y)));
        for (x, y) in pixels {
            for (total, value) in sum.iter_mut().zip(self.pixel(x, y)) {
                *total += u64::from(value);
            }
        }
        let count = u64::from((bottom - top) * (right - left)).max(1);
        sum.map(|total| u8::try_from(total / count).unwrap_or(u8::MAX))
    }

    /// The raster as a PNG.
    #[must_use]
    pub fn png(&self) -> Vec<u8> {
        super::png::encode_rgb(self.width, self.height, &self.rgb)
    }
}

/// The source pixels `[start, end)` that new pixel `index` of `new` covers out of `old`.
fn span(index: u32, new: u32, old: u32) -> (u32, u32) {
    let start = u32::try_from(u64::from(index) * u64::from(old) / u64::from(new)).unwrap_or(old);
    let end = u32::try_from(u64::from(index + 1) * u64::from(old) / u64::from(new)).unwrap_or(old);
    (start.min(old - 1), end.clamp(start + 1, old))
}

/// A 24- or 32-bit pixel's value, in the file's byte order.
fn word(pixel: &[u8], little: bool) -> u32 {
    let mut word = [0_u8; 4];
    word[..pixel.len()].copy_from_slice(pixel);
    if little {
        u32::from_le_bytes(word)
    } else {
        u32::from_be_bytes(word) >> (8 * (4 - pixel.len()))
    }
}

/// One 8-bit channel of a pixel by its mask.
fn channel(value: u32, mask: u32) -> u8 {
    let shifted = (value & mask) >> mask.trailing_zeros();
    let width = (mask >> mask.trailing_zeros()).count_ones();
    let scaled = if width >= 8 {
        shifted >> (width - 8)
    } else {
        shifted << (8 - width)
    };
    u8::try_from(scaled & 0xff).unwrap_or(u8::MAX)
}
