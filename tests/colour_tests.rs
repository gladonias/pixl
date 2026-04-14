//! Unit tests for colour format conversions.
//!
//! Tests cover hex, rgb, hsl, and hsv output for: pure black, pure white,
//! primary red, and a mid-tone value (128, 64, 192).
//! No display or network access required.

use pixl::colour::RgbColour;

// --- hex ---

#[test]
fn hex_black() {
    assert_eq!(RgbColour { r: 0, g: 0, b: 0 }.to_hex(), "#000000");
}

#[test]
fn hex_white() {
    assert_eq!(
        RgbColour {
            r: 255,
            g: 255,
            b: 255
        }
        .to_hex(),
        "#ffffff"
    );
}

#[test]
fn hex_red() {
    assert_eq!(RgbColour { r: 255, g: 0, b: 0 }.to_hex(), "#ff0000");
}

#[test]
fn hex_midtone() {
    assert_eq!(
        RgbColour {
            r: 128,
            g: 64,
            b: 192
        }
        .to_hex(),
        "#8040c0"
    );
}

// --- rgb ---

#[test]
fn rgb_black() {
    assert_eq!(
        RgbColour { r: 0, g: 0, b: 0 }.to_rgb_string(),
        "rgb(0, 0, 0)"
    );
}

#[test]
fn rgb_white() {
    assert_eq!(
        RgbColour {
            r: 255,
            g: 255,
            b: 255
        }
        .to_rgb_string(),
        "rgb(255, 255, 255)"
    );
}

#[test]
fn rgb_red() {
    assert_eq!(
        RgbColour { r: 255, g: 0, b: 0 }.to_rgb_string(),
        "rgb(255, 0, 0)"
    );
}

#[test]
fn rgb_midtone() {
    assert_eq!(
        RgbColour {
            r: 128,
            g: 64,
            b: 192
        }
        .to_rgb_string(),
        "rgb(128, 64, 192)"
    );
}

// --- hsl ---
// Reference values verified against https://www.rapidtables.com/convert/color/rgb-to-hsl.html

#[test]
fn hsl_black() {
    assert_eq!(
        RgbColour { r: 0, g: 0, b: 0 }.to_hsl_string(),
        "hsl(0, 0%, 0%)"
    );
}

#[test]
fn hsl_white() {
    assert_eq!(
        RgbColour {
            r: 255,
            g: 255,
            b: 255
        }
        .to_hsl_string(),
        "hsl(0, 0%, 100%)"
    );
}

#[test]
fn hsl_red() {
    assert_eq!(
        RgbColour { r: 255, g: 0, b: 0 }.to_hsl_string(),
        "hsl(0, 100%, 50%)"
    );
}

#[test]
fn hsl_midtone() {
    // RGB(128, 64, 192) => H=270, S=50%, L=50%
    assert_eq!(
        RgbColour {
            r: 128,
            g: 64,
            b: 192
        }
        .to_hsl_string(),
        "hsl(270, 50%, 50%)"
    );
}

// --- hsv ---
// Reference values verified against https://www.rapidtables.com/convert/color/rgb-to-hsv.html

#[test]
fn hsv_black() {
    assert_eq!(
        RgbColour { r: 0, g: 0, b: 0 }.to_hsv_string(),
        "hsv(0, 0%, 0%)"
    );
}

#[test]
fn hsv_white() {
    assert_eq!(
        RgbColour {
            r: 255,
            g: 255,
            b: 255
        }
        .to_hsv_string(),
        "hsv(0, 0%, 100%)"
    );
}

#[test]
fn hsv_red() {
    assert_eq!(
        RgbColour { r: 255, g: 0, b: 0 }.to_hsv_string(),
        "hsv(0, 100%, 100%)"
    );
}

#[test]
fn hsv_midtone() {
    // RGB(128, 64, 192) => H=270, S=67%, V=75%
    assert_eq!(
        RgbColour {
            r: 128,
            g: 64,
            b: 192
        }
        .to_hsv_string(),
        "hsv(270, 67%, 75%)"
    );
}

// --- round-trip ---

#[test]
fn roundtrip_hsl() {
    let original = RgbColour {
        r: 128,
        g: 64,
        b: 192,
    };
    let (h, s, l) = original.to_hsl();

    // Convert HSL back to RGB
    let s_f = s as f64 / 100.0;
    let l_f = l as f64 / 100.0;
    let c = (1.0 - (2.0 * l_f - 1.0).abs()) * s_f;
    let h_prime = h as f64 / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l_f - c / 2.0;

    let (r1, g1, b1) = match h_prime as u16 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    let r = ((r1 + m) * 255.0).round() as i16;
    let g = ((g1 + m) * 255.0).round() as i16;
    let b = ((b1 + m) * 255.0).round() as i16;

    assert!(
        (r - original.r as i16).abs() <= 1,
        "r: got {r}, expected {}",
        original.r
    );
    assert!(
        (g - original.g as i16).abs() <= 1,
        "g: got {g}, expected {}",
        original.g
    );
    assert!(
        (b - original.b as i16).abs() <= 1,
        "b: got {b}, expected {}",
        original.b
    );
}
