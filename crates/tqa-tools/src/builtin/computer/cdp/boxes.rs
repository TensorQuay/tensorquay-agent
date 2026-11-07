//! Where a page's elements are on the screen (ADR-0039 slice 3, set-of-marks): one `DOMSnapshot.captureSnapshot` gives
//! every laid-out node's box in the document, and the window's own metrics (read once a snapshot) map them to the
//! screen. A box outside the page's visible part is cut to it, or dropped when none of it shows.
use std::collections::HashMap;

use serde_json::Value;

use crate::builtin::computer::raster::Rect;

/// The script that reads the window's metrics, in [`Viewport::from_metrics`]'s order.
pub const METRICS: &str = "[screenX,screenY,outerWidth,outerHeight,innerWidth,innerHeight,scrollX,scrollY,\
                           devicePixelRatio]";

/// The page's visible part on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Its left edge on the screen, in screen pixels.
    pub left: f64,
    /// Its top edge.
    pub top: f64,
    /// Its width, in CSS pixels.
    pub width: f64,
    /// Its height, in CSS pixels.
    pub height: f64,
    /// How far the page is scrolled across, in CSS pixels.
    pub scroll_x: f64,
    /// How far down.
    pub scroll_y: f64,
    /// Screen pixels per CSS pixel.
    pub scale: f64,
}

impl Viewport {
    /// The viewport from [`METRICS`]' answer. The browser's own frame is the difference between the window's outer
    /// and inner sizes: an even border on each side, and the rest of the height (its tabs and toolbar) above.
    #[must_use]
    pub fn from_metrics(metrics: &Value) -> Option<Self> {
        let number = |index: usize| metrics.get(index).and_then(Value::as_f64);
        let (screen_x, screen_y) = (number(0)?, number(1)?);
        let (outer_width, outer_height) = (number(2)?, number(3)?);
        let (width, height) = (number(4)?, number(5)?);
        let scale = number(8).filter(|scale| *scale > 0.0).unwrap_or(1.0);
        let border = ((outer_width - width * scale) / 2.0).max(0.0);
        Some(Self {
            left: screen_x + border,
            top: screen_y + (outer_height - height * scale - border).max(0.0),
            width,
            height,
            scroll_x: number(6)?,
            scroll_y: number(7)?,
            scale,
        })
    }

    /// A document box (x, y, width, height in CSS pixels) on the screen, cut to the visible part.
    #[must_use]
    pub fn on_screen(&self, bounds: [f64; 4]) -> Option<Rect> {
        let [x, y, width, height] = bounds;
        let left = (x - self.scroll_x).max(0.0);
        let top = (y - self.scroll_y).max(0.0);
        let right = (x - self.scroll_x + width).min(self.width);
        let bottom = (y - self.scroll_y + height).min(self.height);
        if right <= left || bottom <= top {
            return None;
        }
        let screen = |value: f64, origin: f64| origin + value * self.scale;
        let (x0, y0) = (screen(left, self.left), screen(top, self.top));
        let (x1, y1) = (screen(right, self.left), screen(bottom, self.top));
        Rect::new(
            pixels(x0.floor())?,
            pixels(y0.floor())?,
            u32::try_from(pixels(x1.ceil())? - pixels(x0.floor())?).ok()?,
            u32::try_from(pixels(y1.ceil())? - pixels(y0.floor())?).ok()?,
        )
    }
}

/// The main document's laid-out nodes by backend node id, each with its box on the screen.
#[must_use]
pub fn boxes(snapshot: &Value, viewport: &Viewport) -> HashMap<i64, Rect> {
    let document = &snapshot["documents"][0];
    let ids = document["nodes"]["backendNodeId"].as_array();
    let indices = document["layout"]["nodeIndex"].as_array();
    let bounds = document["layout"]["bounds"].as_array();
    let (Some(ids), Some(indices), Some(bounds)) = (ids, indices, bounds) else {
        return HashMap::new();
    };
    let mut found = HashMap::new();
    for (index, bound) in indices.iter().zip(bounds) {
        let Some(id) = index
            .as_u64()
            .and_then(|index| ids.get(usize::try_from(index).ok()?))
            .and_then(Value::as_i64)
        else {
            continue;
        };
        let Some(bound) = bound.as_array().filter(|bound| bound.len() == 4) else {
            continue;
        };
        let values: Vec<f64> = bound.iter().filter_map(Value::as_f64).collect();
        let Ok(values) = <[f64; 4]>::try_from(values) else {
            continue;
        };
        if let Some(rect) = viewport.on_screen(values) {
            // A node laid out in several boxes (a wrapped inline) keeps its first.
            found.entry(id).or_insert(rect);
        }
    }
    found
}

/// A screen coordinate as whole pixels, within `i32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the value is finite and checked to be within i32 before the cast"
)]
fn pixels(value: f64) -> Option<i32> {
    (value.is_finite() && value.abs() < f64::from(i32::MAX)).then(|| value.round() as i32)
}
