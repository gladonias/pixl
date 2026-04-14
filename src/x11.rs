//! X11 pixel capture path.
//!
//! Uses `x11rb` to grab the pointer with a crosshair cursor, wait for
//! a button press, and read the pixel at the clicked position using
//! `GetImageRequest`. Mirrors the behaviour of grabc.

use crate::colour::RgbColour;
use crate::error::PixlError;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, ConnectionExt, CreateWindowAux, EventMask, GrabStatus, ImageFormat, Screen, Visualtype,
    WindowClass,
};
use x11rb::COPY_DEPTH_FROM_PARENT;

const MAG_SIZE: u16 = 150;
const MAG_PIXELS: i16 = 5; // half-extent: 11x11 capture region
const MAG_SCALE: u16 = MAG_SIZE / 11;
const MAG_OFFSET: i16 = 20;

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
    // Crosshair cursor glyph = 34
    conn.create_glyph_cursor(cursor, font_id, font_id, 34, 35, 0xffff, 0xffff, 0xffff, 0, 0, 0)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.close_font(font_id)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    let grab = conn
        .grab_pointer(
            false,
            root,
            (EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION).into(),
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

    // Create magnifier window if enabled
    let mag_state = if magnifier {
        create_magnifier(&conn, screen).ok()
    } else {
        None
    };

    // Event loop: wait for click, update magnifier on motion
    let (px, py) = loop {
        let event = conn
            .wait_for_event()
            .map_err(|e| PixlError::X11Error(e.to_string()))?;
        match event {
            x11rb::protocol::Event::ButtonPress(ev) => {
                break (ev.root_x as i16, ev.root_y as i16);
            }
            x11rb::protocol::Event::MotionNotify(ev) => {
                if let Some(ref ms) = mag_state {
                    let _ = update_magnifier(
                        &conn, screen, &visual, ms, ev.root_x as i16, ev.root_y as i16,
                    );
                }
            }
            _ => {}
        }
    };

    // Clean up magnifier
    if let Some(ref ms) = mag_state {
        let _ = conn.unmap_window(ms.win);
        let _ = conn.flush();
    }

    conn.ungrab_pointer(x11rb::CURRENT_TIME)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.flush()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    // Capture the 1x1 pixel
    let reply = conn.get_image(
        ImageFormat::Z_PIXMAP,
        root,
        px,
        py,
        1,
        1,
        u32::MAX,
    )
    .map_err(|e| PixlError::CaptureError(e.to_string()))?
    .reply()
    .map_err(|e| PixlError::CaptureError(e.to_string()))?;

    pixel_from_image_data(&reply.data, reply.depth, &visual, &conn)
}

struct MagState {
    win: u32,
    gc: u32,
}

fn create_magnifier(
    conn: &impl Connection,
    screen: &Screen,
) -> Result<MagState, PixlError> {
    let win = conn
        .generate_id()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    let gc = conn
        .generate_id()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        0,
        0,
        MAG_SIZE,
        MAG_SIZE,
        1,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE),
    )
    .map_err(|e| PixlError::X11Error(e.to_string()))?;

    conn.create_gc(gc, win, &xproto::CreateGCAux::new())
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    conn.map_window(win)
        .map_err(|e| PixlError::X11Error(e.to_string()))?;
    conn.flush()
        .map_err(|e| PixlError::X11Error(e.to_string()))?;

    Ok(MagState { win, gc })
}

fn update_magnifier(
    conn: &impl Connection,
    screen: &Screen,
    _visual: &Visualtype,
    ms: &MagState,
    cx: i16,
    cy: i16,
) -> Result<(), PixlError> {
    let sw = screen.width_in_pixels as i16;
    let sh = screen.height_in_pixels as i16;

    // Position the magnifier window offset from cursor
    let wx = if cx + MAG_OFFSET + MAG_SIZE as i16 > sw {
        cx - MAG_OFFSET - MAG_SIZE as i16
    } else {
        cx + MAG_OFFSET
    };
    let wy = if cy + MAG_OFFSET + MAG_SIZE as i16 > sh {
        cy - MAG_OFFSET - MAG_SIZE as i16
    } else {
        cy + MAG_OFFSET
    };

    conn.configure_window(
        ms.win,
        &xproto::ConfigureWindowAux::new().x(wx as i32).y(wy as i32),
    )
    .map_err(|e| PixlError::X11Error(e.to_string()))?;

    // Capture 11x11 region around cursor, clamped to screen bounds
    let cap_x = (cx - MAG_PIXELS).max(0);
    let cap_y = (cy - MAG_PIXELS).max(0);
    let cap_w = ((MAG_PIXELS * 2 + 1) as u16).min((sw - cap_x) as u16);
    let cap_h = ((MAG_PIXELS * 2 + 1) as u16).min((sh - cap_y) as u16);

    let img = match conn
        .get_image(ImageFormat::Z_PIXMAP, screen.root, cap_x, cap_y, cap_w, cap_h, u32::MAX)
    {
        Ok(cookie) => match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => return Ok(()),
        },
        Err(_) => return Ok(()),
    };

    let depth = img.depth;
    let bpp = if depth > 16 { 4usize } else if depth > 8 { 2 } else { 1 };
    let row_stride = cap_w as usize * bpp;

    // Build magnified image: each source pixel maps to MAG_SCALE x MAG_SCALE
    let out_w = MAG_SIZE as usize;
    let out_h = MAG_SIZE as usize;
    let out_bpp = 4usize; // always use 32-bit for output
    let out_stride = out_w * out_bpp;
    let mut out_data = vec![0u8; out_h * out_stride];

    for oy in 0..out_h {
        let sy = oy / MAG_SCALE as usize;
        for ox in 0..out_w {
            let sx = ox / MAG_SCALE as usize;
            if sy < cap_h as usize && sx < cap_w as usize {
                let src_off = sy * row_stride + sx * bpp;
                let dst_off = oy * out_stride + ox * out_bpp;
                if src_off + bpp <= img.data.len() && dst_off + out_bpp <= out_data.len() {
                    // Copy pixel, padding to 32-bit if needed
                    for i in 0..bpp.min(out_bpp) {
                        out_data[dst_off + i] = img.data[src_off + i];
                    }
                    if bpp < out_bpp {
                        out_data[dst_off + 3] = 0xff;
                    }
                }
            }
        }
    }

    // Draw crosshair in the centre pixel block
    let centre_x = MAG_PIXELS as usize * MAG_SCALE as usize;
    let centre_y = MAG_PIXELS as usize * MAG_SCALE as usize;
    for i in 0..MAG_SCALE as usize {
        // Horizontal line through centre
        let off = centre_y * out_stride + (centre_x + i) * out_bpp;
        if off + 2 < out_data.len() {
            out_data[off] = 0xff;
            out_data[off + 1] = 0x00;
            out_data[off + 2] = 0x00;
        }
        // Vertical line through centre
        let off = (centre_y + i) * out_stride + centre_x * out_bpp;
        if off + 2 < out_data.len() {
            out_data[off] = 0xff;
            out_data[off + 1] = 0x00;
            out_data[off + 2] = 0x00;
        }
    }

    let _ = conn.put_image(
        ImageFormat::Z_PIXMAP,
        ms.win,
        ms.gc,
        out_w as u16,
        out_h as u16,
        0,
        0,
        0,
        screen.root_depth,
        &out_data,
    );
    let _ = conn.flush();

    Ok(())
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
    _conn: &impl Connection,
) -> Result<RgbColour, PixlError> {
    if data.is_empty() {
        return Err(PixlError::CaptureError("empty image data".into()));
    }

    if depth >= 24 && data.len() >= 4 {
        // 32/24-bit TrueColor: data is typically BGRA or BGRX in Z_PIXMAP
        // Extract using visual masks
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
    // Scale to 8-bit
    if bits >= 8 {
        (val >> (bits - 8)) as u8
    } else {
        ((val << (8 - bits)) | (val >> (2 * bits - 8).min(bits))) as u8
    }
}
