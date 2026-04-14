//! xdg-desktop-portal colour picker path.
//!
//! Uses `org.freedesktop.portal.Screenshot.PickColor` over D-Bus.
//! Works on KDE Plasma Wayland and GNOME Wayland via their respective
//! portal implementations.

use crate::colour::RgbColour;
use crate::error::PixlError;

pub fn pick_colour() -> Result<RgbColour, PixlError> {
    Err(PixlError::PortalUnavailable("not yet implemented".into()))
}
