//! wlroots screencopy capture path.
//!
//! Uses `zwlr_screencopy_manager_v1` to capture a full-screen snapshot, then
//! `zwlr_layer_shell_v1` for an overlay surface that renders a magnifier lens
//! following the pointer. On click, reads the pixel from the snapshot.

use crate::colour::RgbColour;
use crate::error::PixlError;
use crate::magnifier;
use std::os::unix::io::{AsFd, AsRawFd};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

fn wl_err(e: impl std::fmt::Display) -> PixlError {
    PixlError::WaylandError(e.to_string())
}

pub fn pick_colour(use_magnifier: bool) -> Result<RgbColour, PixlError> {
    let conn = Connection::connect_to_env().map_err(wl_err)?;
    let mut queue: EventQueue<State> = conn.new_event_queue();
    let qh = queue.handle();
    let mut st = State::default();

    let _reg = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut st).map_err(wl_err)?;

    if st.screencopy.is_none() || st.layer_shell.is_none() {
        return Err(PixlError::ProtocolUnsupported);
    }
    let compositor = st
        .compositor
        .clone()
        .ok_or_else(|| wl_err("no wl_compositor"))?;
    let shm = st.shm.clone().ok_or_else(|| wl_err("no wl_shm"))?;
    let seat = st.seat.clone().ok_or_else(|| wl_err("no wl_seat"))?;
    let layer_shell = st.layer_shell.clone().unwrap();
    if st.outputs.is_empty() {
        return Err(PixlError::WaylandError("no outputs found".into()));
    }
    let scm = st.screencopy.clone().unwrap();
    let output = st.outputs[0].clone();

    // Phase 1: capture full-screen snapshot (before overlay)
    let snap = if use_magnifier {
        Some(capture_full_output(
            &mut st, &mut queue, &qh, &shm, &scm, &output,
        )?)
    } else {
        None
    };

    // Phase 2: full-screen overlay surface to capture pointer + click globally.
    // We always use a fullscreen layer surface so that pointer motion events
    // fire across the whole screen (not just over the magnifier rectangle) and
    // so that surface-local pointer coordinates match screen coordinates.
    let surface = compositor.create_surface(&qh, ());
    let ls = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Overlay,
        "pixl".into(),
        &qh,
        (),
    );
    ls.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    ls.set_exclusive_zone(-1);
    ls.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    surface.commit();
    queue.roundtrip(&mut st).map_err(wl_err)?;

    let serial = st
        .configure_serial
        .take()
        .ok_or_else(|| wl_err("no layer-surface configure event"))?;
    let (mut w, mut h) = (st.configure_w as i32, st.configure_h as i32);
    // Fallback: some compositors may send (0, 0) asking us to choose; use the
    // snapshot dimensions when available.
    if w <= 0 || h <= 0 {
        if let Some(ref s) = snap {
            w = s.width;
            h = s.height;
        } else {
            return Err(PixlError::WaylandError(format!(
                "invalid configure size {}x{}",
                w, h
            )));
        }
    }
    ls.ack_configure(serial);

    // Allocate a single full-screen ARGB8888 buffer, mmap'd, and reuse it for
    // every redraw. Start fully transparent; the magnifier is painted into a
    // sub-rect on motion events.
    let stride = w * 4;
    let buf_size = (stride as usize) * (h as usize);
    let mut shm_buf = ShmBuffer::new(buf_size)?;
    let pool = shm.create_pool(shm_buf.file.as_fd(), buf_size as i32, &qh, ());
    let buffer = pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, &qh, ());

    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();

    let _ptr = seat.get_pointer(&qh, ());

    st.use_magnifier = use_magnifier;
    st.mag_needs_redraw = false;
    let mut prev_mag: Option<(i32, i32)> = None;

    loop {
        queue.blocking_dispatch(&mut st).map_err(wl_err)?;

        if st.use_magnifier && st.mag_needs_redraw {
            st.mag_needs_redraw = false;
            if let Some(ref s) = snap {
                let cx = st.ptr_x as i32;
                let cy = st.ptr_y as i32;
                let (mx, my) = magnifier::position(cx, cy, w, h);
                let rendered = magnifier::render(s, cx, cy);
                let stride_usize = stride as usize;

                let slice = shm_buf.as_mut_slice();
                if let Some((ox, oy)) = prev_mag {
                    clear_rect(slice, stride_usize, ox, oy, magnifier::SIZE, w, h);
                }
                blit_rect(slice, stride_usize, mx, my, magnifier::SIZE, w, h, &rendered);

                surface.attach(Some(&buffer), 0, 0);
                if let Some((ox, oy)) = prev_mag {
                    surface.damage_buffer(ox, oy, magnifier::SIZE, magnifier::SIZE);
                }
                surface.damage_buffer(mx, my, magnifier::SIZE, magnifier::SIZE);
                surface.commit();

                prev_mag = Some((mx, my));
            }
        }

        if st.click.is_some() {
            break;
        }
    }

    buffer.destroy();
    pool.destroy();
    ls.destroy();
    surface.destroy();
    queue.roundtrip(&mut st).map_err(wl_err)?;
    drop(shm_buf);

    let (cx, cy) = st
        .click
        .ok_or_else(|| PixlError::CaptureError("no click".into()))?;

    // Read pixel from snapshot if available
    if let Some(ref s) = snap {
        let x = cx.clamp(0, s.width - 1);
        let y = cy.clamp(0, s.height - 1);
        let off = (y * s.stride + x * s.bpp) as usize;
        if off + 3 <= s.data.len() {
            let (r, g, b) = match s.order {
                magnifier::ChannelOrder::Bgra => (s.data[off + 2], s.data[off + 1], s.data[off]),
                magnifier::ChannelOrder::Rgba => (s.data[off], s.data[off + 1], s.data[off + 2]),
            };
            return Ok(RgbColour { r, g, b });
        }
    }

    // Fallback: capture 1x1 pixel via screencopy
    capture_single_pixel(&mut st, &mut queue, &qh, &shm, &scm, &output, cx, cy)
}

fn capture_full_output(
    st: &mut State,
    queue: &mut EventQueue<State>,
    qh: &QueueHandle<State>,
    shm: &wl_shm::WlShm,
    scm: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    output: &wl_output::WlOutput,
) -> Result<magnifier::Snapshot, PixlError> {
    st.frame_buf = None;
    st.sc_frame = None;
    st.frame_ready = false;
    st.frame_failed = false;

    let _frame = scm.capture_output(0, output, qh, ());
    queue.roundtrip(st).map_err(wl_err)?;

    let fi = st
        .frame_buf
        .take()
        .ok_or_else(|| PixlError::CaptureError("no buffer info from screencopy".into()))?;

    let sz = (fi.stride * fi.height) as i32;
    let fd = create_shm_file(sz as usize)?;
    let pool = shm.create_pool(fd.as_fd(), sz, qh, ());
    let buffer = pool.create_buffer(
        0,
        fi.width as i32,
        fi.height as i32,
        fi.stride as i32,
        fi.format,
        qh,
        (),
    );
    if let Some(ref f) = st.sc_frame {
        f.copy(&buffer);
    }
    loop {
        queue.blocking_dispatch(st).map_err(wl_err)?;
        if st.frame_ready || st.frame_failed {
            break;
        }
    }
    if let Some(f) = st.sc_frame.take() {
        f.destroy();
    }
    buffer.destroy();
    pool.destroy();
    if st.frame_failed {
        return Err(PixlError::CaptureError("screencopy capture failed".into()));
    }

    let mut data = vec![0u8; sz as usize];
    read_shm(&fd, &mut data)?;

    let order = match fi.format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 => magnifier::ChannelOrder::Bgra,
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => magnifier::ChannelOrder::Rgba,
        other => {
            return Err(PixlError::CaptureError(format!(
                "unsupported buffer format: {:?}",
                other
            )))
        }
    };

    Ok(magnifier::Snapshot {
        data,
        width: fi.width as i32,
        height: fi.height as i32,
        bpp: 4,
        stride: fi.stride as i32,
        order,
    })
}

#[allow(clippy::too_many_arguments)]
fn capture_single_pixel(
    st: &mut State,
    queue: &mut EventQueue<State>,
    qh: &QueueHandle<State>,
    shm: &wl_shm::WlShm,
    scm: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    output: &wl_output::WlOutput,
    cx: i32,
    cy: i32,
) -> Result<RgbColour, PixlError> {
    st.frame_buf = None;
    st.sc_frame = None;
    st.frame_ready = false;
    st.frame_failed = false;

    let _frame = scm.capture_output_region(0, output, cx, cy, 1, 1, qh, ());
    queue.roundtrip(st).map_err(wl_err)?;

    let fi = st
        .frame_buf
        .take()
        .ok_or_else(|| PixlError::CaptureError("no buffer info from screencopy".into()))?;
    let sz = (fi.stride * fi.height) as i32;
    let fd = create_shm_file(sz as usize)?;
    let pool = shm.create_pool(fd.as_fd(), sz, qh, ());
    let buffer = pool.create_buffer(
        0,
        fi.width as i32,
        fi.height as i32,
        fi.stride as i32,
        fi.format,
        qh,
        (),
    );
    if let Some(ref f) = st.sc_frame {
        f.copy(&buffer);
    }
    loop {
        queue.blocking_dispatch(st).map_err(wl_err)?;
        if st.frame_ready || st.frame_failed {
            break;
        }
    }
    if let Some(f) = st.sc_frame.take() {
        f.destroy();
    }
    buffer.destroy();
    pool.destroy();
    if st.frame_failed {
        return Err(PixlError::CaptureError("screencopy capture failed".into()));
    }
    read_pixel(&fd, &fi)
}

struct BufInfo {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    screencopy: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    outputs: Vec<wl_output::WlOutput>,
    configure_serial: Option<u32>,
    configure_w: u32,
    configure_h: u32,
    click: Option<(i32, i32)>,
    ptr_x: f64,
    ptr_y: f64,
    frame_buf: Option<BufInfo>,
    sc_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    frame_ready: bool,
    frame_failed: bool,
    use_magnifier: bool,
    mag_needs_redraw: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        st: &mut Self,
        reg: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    st.compositor = Some(reg.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" => {
                    st.shm = Some(reg.bind(name, version.min(1), qh, ()));
                }
                "wl_seat" => {
                    st.seat = Some(reg.bind(name, version.min(9), qh, ()));
                }
                "wl_output" => {
                    st.outputs.push(reg.bind(name, version.min(4), qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    st.layer_shell = Some(reg.bind(name, version.min(4), qh, ()));
                }
                "zwlr_screencopy_manager_v1" => {
                    st.screencopy = Some(reg.bind(name, version.min(3), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        st: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                st.ptr_x = surface_x;
                st.ptr_y = surface_y;
                if st.use_magnifier {
                    st.mag_needs_redraw = true;
                }
            }
            wl_pointer::Event::Button { state: s, .. } => {
                if s == WEnum::Value(wl_pointer::ButtonState::Pressed) {
                    st.click = Some((st.ptr_x as i32, st.ptr_y as i32));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        st: &mut Self,
        _: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            st.configure_serial = Some(serial);
            st.configure_w = width;
            st.configure_h = height;
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        st: &mut Self,
        proxy: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format: WEnum::Value(fmt),
                width,
                height,
                stride,
            } => {
                st.frame_buf = Some(BufInfo {
                    format: fmt,
                    width,
                    height,
                    stride,
                });
                st.sc_frame = Some(proxy.clone());
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                st.frame_ready = true;
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                st.frame_failed = true;
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore wl_output::WlOutput);
delegate_noop!(State: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(State: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);

/// A shm-backed buffer that is both shared with the Wayland compositor
/// (via `wl_shm.create_pool`) and mmap'd in our own address space so we
/// can repaint sub-regions without reallocating.
struct ShmBuffer {
    file: std::fs::File,
    ptr: *mut u8,
    size: usize,
}

impl ShmBuffer {
    fn new(size: usize) -> Result<Self, PixlError> {
        let file = create_shm_file(size)?;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(PixlError::WaylandError(format!(
                "mmap: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self {
            file,
            ptr: ptr as *mut u8,
            size,
        })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut _, self.size);
        }
    }
}

/// Clear a (possibly out-of-bounds) rectangle in an ARGB8888 buffer to zero.
fn clear_rect(buf: &mut [u8], stride: usize, x: i32, y: i32, size: i32, w: i32, h: i32) {
    let xs = x.max(0);
    let ys = y.max(0);
    let xe = (x + size).min(w);
    let ye = (y + size).min(h);
    if xs >= xe || ys >= ye {
        return;
    }
    for row in ys..ye {
        let start = row as usize * stride + xs as usize * 4;
        let end = row as usize * stride + xe as usize * 4;
        for b in &mut buf[start..end] {
            *b = 0;
        }
    }
}

/// Copy a `size × size` ARGB8888 tile `src` into `buf` at (x, y), clipped
/// to the buffer's (w, h) bounds.
#[allow(clippy::too_many_arguments)]
fn blit_rect(
    buf: &mut [u8],
    stride: usize,
    x: i32,
    y: i32,
    size: i32,
    w: i32,
    h: i32,
    src: &[u8],
) {
    let xs = x.max(0);
    let ys = y.max(0);
    let xe = (x + size).min(w);
    let ye = (y + size).min(h);
    if xs >= xe || ys >= ye {
        return;
    }
    let src_stride = size as usize * 4;
    let copy_len = (xe - xs) as usize * 4;
    for row in ys..ye {
        let sy = (row - y) as usize;
        let sx = (xs - x) as usize;
        let src_start = sy * src_stride + sx * 4;
        let dst_start = row as usize * stride + xs as usize * 4;
        buf[dst_start..dst_start + copy_len]
            .copy_from_slice(&src[src_start..src_start + copy_len]);
    }
}

fn create_shm_file(size: usize) -> Result<std::fs::File, PixlError> {
    use std::ffi::CString;
    use std::os::unix::io::FromRawFd;

    let name = CString::new(format!("/pixl-shm-{}", std::process::id())).map_err(wl_err)?;
    let fd = unsafe {
        libc::shm_open(
            name.as_ptr(),
            libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
            0o600,
        )
    };
    if fd < 0 {
        return Err(PixlError::WaylandError(format!(
            "shm_open: {}",
            std::io::Error::last_os_error()
        )));
    }
    unsafe { libc::shm_unlink(name.as_ptr()) };
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.set_len(size as u64).map_err(wl_err)?;
    Ok(file)
}

fn read_shm(file: &std::fs::File, buf: &mut [u8]) -> Result<(), PixlError> {
    use std::io::{Read, Seek};
    let mut f = file;
    f.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;
    f.read_exact(buf)
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;
    Ok(())
}

fn read_pixel(file: &std::fs::File, info: &BufInfo) -> Result<RgbColour, PixlError> {
    let mut buf = vec![0u8; (info.stride * info.height) as usize];
    read_shm(file, &mut buf)?;
    match info.format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 if buf.len() >= 4 => Ok(RgbColour {
            b: buf[0],
            g: buf[1],
            r: buf[2],
        }),
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 if buf.len() >= 4 => Ok(RgbColour {
            r: buf[0],
            g: buf[1],
            b: buf[2],
        }),
        other => Err(PixlError::CaptureError(format!(
            "unsupported buffer format: {:?}",
            other
        ))),
    }
}
