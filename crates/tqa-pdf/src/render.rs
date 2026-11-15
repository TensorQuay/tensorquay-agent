//! A page as a picture (PRD item 30: a route with `vision` gets page pictures): hayro draws it at a given resolution
//! on a white ground and encodes a PNG. The standard 14 fonts are built in; a font the file doesn't embed and hayro
//! doesn't carry draws as hayro's fallback, so a picture is always made, if not always with the right letterforms.
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

use crate::Document;

/// The resolution a page picture is drawn at: the benchmark's, legible for body text, about 1,200 by 1,700 for A4.
pub const DPI: f32 = 144.0;
/// A PDF's own unit is a point, 72 to the inch.
const POINTS_PER_INCH: f32 = 72.0;
/// The most bytes a page's PNG may take: a model's image limit (`MAX_IMAGE_BYTES`) with room for the request around it.
pub const MAX_PNG_BYTES: usize = 4 * 1024 * 1024;

/// The most pixels a page picture has on its longer side: an A4 page at [`DPI`] is 1,191 by 1,684, so ordinary pages
/// keep their resolution, and a poster, a map or a hostile `MediaBox` (14,400 pt a side) is drawn smaller instead of
/// taking gigabytes before any check (the early review of 15 Nov 2026).
pub const MAX_PAGE_SIDE: f32 = 2_000.0;

/// Why a page couldn't be drawn.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The renderer couldn't open the document (it is more particular than the text engine about some damage).
    #[error("the renderer can't open the document")]
    Unopenable,
    /// The page is past the end, or the renderer sees fewer pages than the text engine.
    #[error("no page {0} to draw")]
    NoSuchPage(u32),
    /// The page has no usable size (zero, negative or not a number).
    #[error("page {0} has no usable size")]
    NoSize(u32),
    /// The PNG couldn't be encoded.
    #[error("the picture couldn't be encoded")]
    Encoding,
    /// The PNG is larger than [`MAX_PNG_BYTES`].
    #[error("the picture is too large")]
    TooLarge,
}

impl Document {
    /// Draws `page` (from 1) as a PNG: [`render_pngs`](Self::render_pngs) for one page.
    ///
    /// # Errors
    /// [`RenderError`], as its variants say.
    pub fn render_png(&self, page: u32) -> Result<Vec<u8>, RenderError> {
        self.render_pngs(&[page])
            .pop()
            .unwrap_or(Err(RenderError::NoSuchPage(page)))
    }

    /// Draws `pages` (from 1) as PNGs, one answer each in the same order, opening the document and its render cache
    /// once for all of them. Each page is drawn at [`DPI`], or smaller so that its longer side is at most
    /// [`MAX_PAGE_SIDE`] pixels, on a white ground.
    #[must_use]
    pub fn render_pngs(&self, pages: &[u32]) -> Vec<Result<Vec<u8>, RenderError>> {
        let Ok(pdf) = Pdf::new(self.bytes().to_vec()) else {
            return pages.iter().map(|_| Err(RenderError::Unopenable)).collect();
        };
        let cache = RenderCache::new();
        let settings = InterpreterSettings::default();
        let all = pdf.pages();
        pages
            .iter()
            .map(|&page| {
                let index =
                    usize::try_from(page.checked_sub(1).ok_or(RenderError::NoSuchPage(page))?)
                        .map_err(|_| RenderError::NoSuchPage(page))?;
                let drawn = all.get(index).ok_or(RenderError::NoSuchPage(page))?;
                let (width, height) = drawn.render_dimensions();
                let longest = width.max(height);
                if !(longest.is_finite() && longest > 0.0 && width > 0.0 && height > 0.0) {
                    return Err(RenderError::NoSize(page));
                }
                let scale = (DPI / POINTS_PER_INCH).min(MAX_PAGE_SIDE / longest);
                let pixmap = render(
                    drawn,
                    &cache,
                    &settings,
                    &RenderSettings::default(),
                    &PixmapSettings {
                        x_scale: scale,
                        y_scale: scale,
                        bg_color: WHITE,
                    },
                );
                let png = pixmap.into_png().map_err(|_| RenderError::Encoding)?;
                if png.len() > MAX_PNG_BYTES {
                    return Err(RenderError::TooLarge);
                }
                Ok(png)
            })
            .collect()
    }
}
