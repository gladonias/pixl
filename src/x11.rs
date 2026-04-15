//! X11 pixel capture path.
//!
//! Grabs the pointer with a crosshair cursor, captures a full-screen snapshot,
//! and shows a magnifier overlay that follows the cursor. On click, reads the
//! pixel at the clicked position from the snapshot.

use crate::colour::RgbColour;
use crate::error::PixlError;
use crate::magnifier;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, ConnectionExt, CreateWindowAux, EventMask, GrabStatus, ImageFormat, Screen, Visualtype,
    WindowClass,
};

fn x_err(e: impl std::fmt::Display) -> PixlError {
    PixlError::X11Error(e.to_string())
}

pub fn pick_colour(use_magnifier: bool) -> Result<RgbColour, PixlError> {
    let (conn, screen_num) = x11rb::connect(None).map_err(x_err)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let sw = screen.width_in_pixels as i32;
    let sh = screen.height_in_pixels as i32;

    let visual =
        find_visual(screen, screen.root_visual).ok_or_else(|| x_err("cannot find root visual"))?;

    // Create crosshair cursor
    let font_id = conn.generate_id().map_err(x_err)?;
    conn.open_font(font_id, b"cursor").map_err(x_err)?;
    let cursor = conn.generate_id().map_err(x_err)?;
    conn.create_glyph_cursor(cursor, font_id, font_id, 34, 35, 0xffff, 0xffff, 0xffff, 0, 0, 0)
        .map_err(x_err)?;
    conn.close_font(font_id).map_err(x_err)?;

    // Capture full-screen snapshot before grabbing (so the grab cursor isn't in it)
    let snap = if use_magnifier {
        Some(capture_snapshot(&conn, root, sw, sh, &visual)?)
    } else {
        None
    };

    // Grab pointer
    let event_mask = EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION;
    let grab = conn
        .grab_pointer(
            false,
            root,
            event_mask,
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
            x11rb::NONE,
            cursor,
            x11rb::CURRENT_TIME,
        )
        .map_err(x_err)?
        .reply()
        .map_err(x_err)?;

    if grab.status != GrabStatus::SUCCESS {
        return Err(PixlError::X11Error("failed to grab pointer".into()));
    }

    // Create magnifier overlay window
    let mag_win = if snap.is_some() {
        Some(create_magnifier_window(
            &conn, screen, root, sw, sh,
        )?)
    } else {
        None
    };

    // Create a GC for PutImage
    let gc = if mag_win.is_some() {
        let id = conn.generate_id().map_err(x_err)?;
        conn.create_gc(id, root, &Default::default())
            .map_err(x_err)?;
        Some(id)
    } else {
        None
    };

    // Event loop
    let (px, py) = loop {
        let event = conn.wait_for_event().map_err(x_err)?;
        match event {
            x11rb::protocol::Event::MotionNotify(ev) => {
                if let (Some(ref s), Some(win), Some(gc_id)) = (&snap, mag_win, gc) {
                    let (mx, my) =
                        magnifier::position(ev.root_x as i32, ev.root_y as i32, sw, sh);
                    let buf = magnifier::render(s, ev.root_x as i32, ev.root_y as i32);
                    conn.move_window(win, mx as i16, my as i16).map_err(x_err)?;
                    conn.put_image(
                        ImageFormat::Z_PIXMAP,
                        win,
                        gc_id,
                        magnifier::SIZE as u16,
                        magnifier::SIZE as u16,
                        0,
                        0,
                        0,
                        screen.root_depth,
                        &buf,
                    )
                    .map_err(x_err)?;
                    conn.flush().map_err(x_err)?;
                }
            }
            x11rb::protocol::Event::ButtonPress(ev) => {
                break (ev.root_x as i32, ev.root_y as i32);
            }
            _ => {}
        }
    };

    // Cleanup
    if let Some(win) = mag_win {
        conn.destroy_window(win).map_err(x_err)?;
    }
    conn.ungrab_pointer(x11rb::CURRENT_TIME).map_err(x_err)?;
    conn.flush().map_err(x_err)?;

    // Read pixel from snapshot if available, otherwise capture 1x1
    if let Some(ref s) = snap {
        let x = px.clamp(0, s.width - 1);
        let y = py.clamp(0, s.height - 1);
        let off = (y * s.stride + x * s.bpp) as usize;
        if off + 3 <= s.data.len() {
            let (r, g, b) = match s.order {
                magnifier::ChannelOrder::Bgra => (s.data[off + 2], s.data[off + 1], s.data[off]),
                magnifier::ChannelOrder::Rgba => (s.data[off], s.data[off + 1], s.data[off + 2]),
            };
            return Ok(RgbColour { r, g, b });
        }
    }

    // Fallback: capture 1x1 pixel directly
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            px as i16,
            py as i16,
            1,
            1,
            u32::MAX,
        )
        .map_err(|e| PixlError::CaptureError(e.to_string()))?
        .reply()
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;

    pixel_from_image_data(&reply.data, reply.depth, &visual)
}

fn capture_snapshot(
    conn: &impl Connection,
    root: u32,
    w: i32,
    h: i32,
    visual: &Visualtype,
) -> Result<magnifier::Snapshot, PixlError> {
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, root, 0, 0, w as u16, h as u16, u32::MAX)
        .map_err(|e| PixlError::CaptureError(e.to_string()))?
        .reply()
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;

    let depth = reply.depth;
    let bpp = if depth >= 24 { 4 } else { 2 };

    // Determine channel order from visual masks
    let order = if visual.red_mask > visual.blue_mask {
        // Red is in higher bits → little-endian stores B first → BGRA
        magnifier::ChannelOrder::Bgra
    } else {
        magnifier::ChannelOrder::Rgba
    };

    // X11 pads rows to the scanline_pad boundary
    let bits_per_pixel = if depth >= 24 { 32 } else { 16 };
    let scanline_pad = conn.setup().bitmap_format_scanline_pad as i32;
    let stride = ((w * bits_per_pixel + scanline_pad - 1) / scanline_pad) * (scanline_pad / 8);

    Ok(magnifier::Snapshot {
        data: reply.data,
        width: w,
        height: h,
        bpp,
        stride,
        order,
    })
}

fn create_magnifier_window(
    conn: &impl Connection,
    screen: &Screen,
    root: u32,
    sw: i32,
    sh: i32,
) -> Result<u32, PixlError> {
    let win = conn.generate_id().map_err(x_err)?;
    let size = magnifier::SIZE as u16;
    // Start off-screen; repositioned on first motion event
    let (mx, my) = magnifier::position(sw / 2, sh / 2, sw, sh);
    conn.create_window(
        screen.root_depth,
        win,
        root,
        mx as i16,
        my as i16,
        size,
        size,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE),
    )
    .map_err(x_err)?;
    conn.map_window(win).map_err(x_err)?;
    conn.flush().map_err(x_err)?;
    Ok(win)
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

/// Extension trait for moving a window (configure request).
trait MoveWindow {
    fn move_window(&self, win: u32, x: i16, y: i16) -> Result<(), PixlError>;
}

impl<C: Connection> MoveWindow for C {
    fn move_window(&self, win: u32, x: i16, y: i16) -> Result<(), PixlError> {
        self.configure_window(
            win,
            &xproto::ConfigureWindowAux::new().x(x as i32).y(y as i32),
        )
        .map_err(x_err)?;
        Ok(())
    }
}
