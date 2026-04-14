//! X11 pixel capture path.
//!
//! Uses `x11rb` to grab the pointer with a crosshair cursor, wait for
//! a button press, and read the pixel at the clicked position using
//! `GetImageRequest`. Mirrors the behaviour of grabc.

use crate::colour::RgbColour;
use crate::error::PixlError;

pub fn pick_colour(_magnifier: bool) -> Result<RgbColour, PixlError> {
    Err(PixlError::X11Error("not yet implemented".into()))
}
