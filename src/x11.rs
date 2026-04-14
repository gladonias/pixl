//! X11 pixel capture path.
//!
//! Uses `x11rb` to grab the pointer with a crosshair cursor, wait for
//! a button press, and read the pixel at the clicked position using
//! `GetImageRequest`. Mirrors the behaviour of grabc.

use crate::colour::RgbColour;
use crate::error::PixlError;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, ConnectionExt, EventMask, GrabStatus, ImageFormat, Screen, Visualtype,
};

pub fn pick_colour(magnifier: bool) -> Result<RgbColour, PixlError> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| PixlError::X11Error(e.to_string()))?;

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let visual = find_visual(screen, screen.root_visual)
        .ok_or_else(|| PixlError::X11Error("cannot find root visual".into()))?;

    // Grab the pointer with a crosshair cursor
    let font_id = conn
        .generate_id()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.open_font(font_id, b"cursor")
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    let cursor = conn
        .generate_id()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.create_glyph_cursor(
        cursor, font_id, font_id, 34, 35, 0xffff, 0xffff, 0xffff, 0, 0, 0,
    )
    .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.close_font(font_id)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    let grab = conn
        .grab_pointer(
            false,
            root,
            EventMask::BUTTON_PRESS,
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
            x11rb::NONE,
            cursor,
            x11rb::CURRENT_TIME,
        )
        .map_err(|e| PixlError::X11Error(e.to_string()))?
        .reply()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    if grab.status != GrabStatus::SUCCESS {
        return Err(PixlError::X11Error("failed to grab pointer".into()));
    }

    // TODO: magnifier overlay (--no-magnifier to disable)
    if magnifier {
        eprintln!("pixl: magnifier not yet implemented for X11, continuing without it");
    }

    // Event loop: wait for click
    let (px, py) = loop {
        let event = conn
            .wait_for_event()
            .map_err(|e| PixlError::X11Error(e.to_string()))?;
        if let x11rb::protocol::Event::ButtonPress(ev) = event {
            break (ev.root_x, ev.root_y);
        }
    };

    conn.ungrab_pointer(x11rb::CURRENT_TIME)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.flush()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    // Capture the 1x1 pixel
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, root, px, py, 1, 1, u32::MAX)
        .map_err(|e| PixlError::CaptureError(e.to_string()))?
        .reply()
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;

    pixel_from_image_data(&reply.data, reply.depth, &visual)
}

fn find_visual(screen: &Screen, visual_id: u32) -> Option<Visualtype> {
    for depth in &screen.allowed_depths {
        for visual in &depth.visuals {
            if visual.visual_id == visual_id {
                return Some(*visual);
            }
        }
    }
    None
}

fn pixel_from_image_data(
    data: &[u8],
    depth: u8,
    visual: &Visualtype,
) -> Result<RgbColour, PixlError> {
    if data.is_empty() {
        return Err(PixlError::CaptureError("empty image data".into()));
    }

    if depth >= 24 && data.len() >= 4 {
        let pixel = u32::from_ne_bytes([data[0], data[1], data[2], data[3]]);
        let r = extract_channel(pixel, visual.red_mask);
        let g = extract_channel(pixel, visual.green_mask);
        let b = extract_channel(pixel, visual.blue_mask);
        Ok(RgbColour { r, g, b })
    } else if depth == 16 && data.len() >= 2 {
        let pixel = u16::from_ne_bytes([data[0], data[1]]) as u32;
        let r = extract_channel(pixel, visual.red_mask);
        let g = extract_channel(pixel, visual.green_mask);
        let b = extract_channel(pixel, visual.blue_mask);
        Ok(RgbColour { r, g, b })
    } else {
        Err(PixlError::CaptureError(format!(
            "unsupported depth {} with {} bytes of data",
            depth,
            data.len()
        )))
    }
}

fn extract_channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    let val = (pixel & mask) >> shift;
    if bits >= 8 {
        (val >> (bits - 8)) as u8
    } else {
        ((val << (8 - bits)) | (val >> (2 * bits - 8).min(bits))) as u8
    }
}
