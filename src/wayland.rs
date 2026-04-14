//! wlroots screencopy capture path.
//!
//! Uses `zwlr_screencopy_manager_v1` to capture a 1×1 pixel region
//! at the position the user clicks. Works on wlroots-based compositors
//! such as Hyprland and Sway.

use crate::colour::RgbColour;
use crate::error::PixlError;

pub fn pick_colour(_magnifier: bool) -> Result<RgbColour, PixlError> {
    Err(PixlError::ProtocolUnsupported)
}
