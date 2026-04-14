//! Colour format conversions.
//!
//! All three capture paths (wlroots, portal, X11) return an `RgbColour`.
//! This module converts that value into the user-requested output format:
//! hex, rgb, hsl, or hsv.

pub struct RgbColour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColour {
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_rgb_string(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    pub fn to_hsl_string(&self) -> String {
        todo!("implement in Layer 1")
    }

    pub fn to_hsv_string(&self) -> String {
        todo!("implement in Layer 1")
    }
}
