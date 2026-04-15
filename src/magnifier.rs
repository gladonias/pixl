//! Shared magnifier rendering.
//!
//! Takes a screen snapshot buffer and a cursor position, produces an ARGB8888
//! pixel buffer showing a zoomed grid of pixels with a crosshair at the center.

/// How many source pixels in each direction from the cursor to show.
const HALF: i32 = 7;
/// Side length of the source region (2*HALF + 1 = 15).
const GRID: i32 = 2 * HALF + 1;
/// Each source pixel is scaled to this many output pixels.
const SCALE: i32 = 11;
/// Total output size in pixels (15 * 11 = 165).
pub const SIZE: i32 = GRID * SCALE;

/// A captured screen snapshot that the magnifier indexes into.
pub struct Snapshot {
    pub data: Vec<u8>,
    pub width: i32,
    pub height: i32,
    /// Bytes per pixel (typically 4 for xRGB / ARGB).
    pub bpp: i32,
    /// Bytes per row.
    pub stride: i32,
    /// Channel order within each 4-byte pixel.
    pub order: ChannelOrder,
}

#[derive(Clone, Copy)]
pub enum ChannelOrder {
    /// B G R A (or X) — little-endian ARGB8888 / XRGB8888.
    Bgra,
    /// R G B A (or X) — little-endian ABGR8888 / XBGR8888.
    Rgba,
}

impl Snapshot {
    fn pixel_rgb(&self, x: i32, y: i32) -> (u8, u8, u8) {
        let x = x.clamp(0, self.width - 1);
        let y = y.clamp(0, self.height - 1);
        let off = (y * self.stride + x * self.bpp) as usize;
        if off + 3 > self.data.len() {
            return (0, 0, 0);
        }
        match self.order {
            ChannelOrder::Bgra => (self.data[off + 2], self.data[off + 1], self.data[off]),
            ChannelOrder::Rgba => (self.data[off], self.data[off + 1], self.data[off + 2]),
        }
    }
}

/// Render the magnifier into an ARGB8888 buffer (SIZE × SIZE pixels).
///
/// `cx, cy` — cursor position in snapshot coordinates.
/// Returns a `Vec<u8>` of length `(SIZE * SIZE * 4) as usize`, laid out as
/// rows of little-endian ARGB8888 pixels (B, G, R, 0xFF per pixel).
pub fn render(snap: &Snapshot, cx: i32, cy: i32) -> Vec<u8> {
    let stride = SIZE * 4;
    let mut buf = vec![0u8; (SIZE * stride) as usize];

    let border: (u8, u8, u8) = (80, 80, 80);
    let cross: (u8, u8, u8) = (255, 60, 60);

    for gy in 0..GRID {
        for gx in 0..GRID {
            let sx = cx - HALF + gx;
            let sy = cy - HALF + gy;
            let (r, g, b) = snap.pixel_rgb(sx, sy);

            let is_center = gx == HALF && gy == HALF;
            let on_crosshair = gx == HALF || gy == HALF;

            for py in 0..SCALE {
                for px in 0..SCALE {
                    let ox = gx * SCALE + px;
                    let oy = gy * SCALE + py;
                    let off = (oy * stride + ox * 4) as usize;

                    let at_edge = px == 0 || py == 0;
                    let at_center_border =
                        is_center && (px == SCALE - 1 || py == SCALE - 1);

                    let (pr, pg, pb) = if at_center_border {
                        cross
                    } else if at_edge && on_crosshair {
                        cross
                    } else if at_edge {
                        border
                    } else {
                        (r, g, b)
                    };

                    buf[off] = pb;
                    buf[off + 1] = pg;
                    buf[off + 2] = pr;
                    buf[off + 3] = 0xFF;
                }
            }
        }
    }
    buf
}

/// Compute magnifier window position, offset from the cursor.
/// Flips sides near screen edges so the magnifier stays on-screen.
pub fn position(cx: i32, cy: i32, screen_w: i32, screen_h: i32) -> (i32, i32) {
    let offset = 20;
    let mx = if cx + offset + SIZE < screen_w {
        cx + offset
    } else {
        cx - offset - SIZE
    };
    let my = if cy + offset + SIZE < screen_h {
        cy + offset
    } else {
        cy - offset - SIZE
    };
    (mx.max(0), my.max(0))
}
