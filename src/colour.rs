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
    /// #rrggbb — default output format.
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// rgb(r, g, b) where r/g/b are 0-255 integers.
    pub fn to_rgb_string(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    /// hsl(h, s%, l%) where h is 0-360, s and l are 0-100.
    pub fn to_hsl_string(&self) -> String {
        let (h, s, l) = self.to_hsl();
        format!("hsl({}, {}%, {}%)", h, s, l)
    }

    /// hsv(h, s%, v%) where h is 0-360, s and v are 0-100.
    pub fn to_hsv_string(&self) -> String {
        let (h, s, v) = self.to_hsv();
        format!("hsv({}, {}%, {}%)", h, s, v)
    }

    /// Convert to HSL. Returns (h: 0-360, s: 0-100, l: 0-100).
    pub fn to_hsl(&self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let l = (max + min) / 2.0;

        if delta == 0.0 {
            return (0, 0, (l * 100.0).round() as u8);
        }

        let s = if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let h = if (max - r).abs() < f64::EPSILON {
            let segment = (g - b) / delta;
            if segment < 0.0 { segment + 6.0 } else { segment }
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };

        let h = (h * 60.0).round() as u16;
        let s = (s * 100.0).round() as u8;
        let l = (l * 100.0).round() as u8;

        (h, s, l)
    }

    /// Convert to HSV. Returns (h: 0-360, s: 0-100, v: 0-100).
    pub fn to_hsv(&self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let v = max;

        if delta == 0.0 {
            return (0, 0, (v * 100.0).round() as u8);
        }

        let s = delta / max;

        let h = if (max - r).abs() < f64::EPSILON {
            let segment = (g - b) / delta;
            if segment < 0.0 { segment + 6.0 } else { segment }
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };

        let h = (h * 60.0).round() as u16;
        let s = (s * 100.0).round() as u8;
        let v = (v * 100.0).round() as u8;

        (h, s, v)
    }
}
