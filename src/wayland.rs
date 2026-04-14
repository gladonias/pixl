//! wlroots screencopy capture path.
//!
//! Uses `zwlr_layer_shell_v1` for an overlay surface to capture pointer
//! clicks, and `zwlr_screencopy_manager_v1` to capture a 1×1 pixel region
//! at the clicked position. Works on wlroots-based compositors (Hyprland, Sway).
//!
//! The magnifier is marked TODO — continuous screencopy for a live magnifier
//! would exceed the line budget.

use crate::colour::RgbColour;
use crate::error::PixlError;
use std::os::unix::io::AsFd;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

fn map_err(e: impl std::fmt::Display) -> PixlError {
    PixlError::WaylandError(e.to_string())
}

pub fn pick_colour(_magnifier: bool) -> Result<RgbColour, PixlError> {
    let conn = Connection::connect_to_env().map_err(map_err)?;
    let display = conn.display();

    let mut event_queue: EventQueue<State> = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut state = State::new();

    // Registry roundtrip to bind globals
    let _registry = display.get_registry(&qh, ());
    event_queue.roundtrip(&mut state).map_err(map_err)?;

    // Verify required globals
    if state.screencopy.is_none() {
        return Err(PixlError::ProtocolUnsupported);
    }
    if state.compositor.is_none() {
        return Err(PixlError::WaylandError("wl_compositor not found".into()));
    }
    if state.layer_shell.is_none() {
        return Err(PixlError::ProtocolUnsupported);
    }
    if state.shm.is_none() {
        return Err(PixlError::WaylandError("wl_shm not found".into()));
    }
    if state.outputs.is_empty() {
        return Err(PixlError::WaylandError("no outputs found".into()));
    }
    if state.seat.is_none() {
        return Err(PixlError::WaylandError("no seat found".into()));
    }

    // Phase 1: Create overlay surface to capture click
    phase_capture_click(&mut state, &mut event_queue, &qh)?;

    let (cx, cy) = state.click.ok_or_else(|| {
        PixlError::CaptureError("no click recorded".into())
    })?;

    // Phase 2: Use screencopy to capture the pixel
    phase_screencopy(&mut state, &mut event_queue, &qh, cx, cy)
}

fn phase_capture_click(
    state: &mut State,
    event_queue: &mut EventQueue<State>,
    qh: &QueueHandle<State>,
) -> Result<(), PixlError> {
    let compositor = state.compositor.as_ref().ok_or_else(|| {
        PixlError::WaylandError("no compositor".into())
    })?.clone();
    let layer_shell = state.layer_shell.as_ref().ok_or_else(|| {
        PixlError::WaylandError("no layer shell".into())
    })?.clone();
    let shm = state.shm.as_ref().ok_or_else(|| {
        PixlError::WaylandError("no shm".into())
    })?.clone();
    let seat = state.seat.as_ref().ok_or_else(|| {
        PixlError::WaylandError("no seat".into())
    })?.clone();

    let surface = compositor.create_surface(qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Overlay,
        "pixl".into(),
        qh,
        (),
    );
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );
    surface.commit();

    event_queue.roundtrip(state).map_err(map_err)?;

    if let Some(serial) = state.layer_configure_serial.take() {
        layer_surface.ack_configure(serial);
        let shm_fd = create_shm_file(4)?;
        let pool = shm.create_pool(shm_fd.as_fd(), 4, qh, ());
        let buffer = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, qh, ());
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
    }

    let _pointer = seat.get_pointer(qh, ());

    loop {
        event_queue.blocking_dispatch(state).map_err(map_err)?;
        if state.click.is_some() {
            break;
        }
    }

    layer_surface.destroy();
    surface.destroy();

    Ok(())
}

fn phase_screencopy(
    state: &mut State,
    event_queue: &mut EventQueue<State>,
    qh: &QueueHandle<State>,
    cx: i32,
    cy: i32,
) -> Result<RgbColour, PixlError> {
    let screencopy = state.screencopy.as_ref().ok_or(PixlError::ProtocolUnsupported)?.clone();
    let shm = state.shm.as_ref().ok_or_else(|| {
        PixlError::WaylandError("no shm".into())
    })?.clone();
    let output = state.outputs.first().ok_or_else(|| {
        PixlError::CaptureError("no output".into())
    })?.clone();

    let _frame = screencopy.capture_output_region(0, &output, cx, cy, 1, 1, qh, ());

    event_queue.roundtrip(state).map_err(map_err)?;

    let buf_info = state.frame_buffer.take().ok_or_else(|| {
        PixlError::CaptureError("no buffer info from screencopy frame".into())
    })?;

    let buf_size = (buf_info.stride * buf_info.height) as i32;
    let shm_fd = create_shm_file(buf_size as usize)?;
    let pool = shm.create_pool(shm_fd.as_fd(), buf_size, qh, ());
    let buffer = pool.create_buffer(
        0,
        buf_info.width as i32,
        buf_info.height as i32,
        buf_info.stride as i32,
        buf_info.format,
        qh,
        (),
    );

    if let Some(ref frame) = state.screencopy_frame {
        frame.copy(&buffer);
    }

    loop {
        event_queue.blocking_dispatch(state).map_err(map_err)?;
        if state.frame_ready || state.frame_failed {
            break;
        }
    }

    if state.frame_failed {
        return Err(PixlError::CaptureError("screencopy frame capture failed".into()));
    }

    read_pixel_from_shm(&shm_fd, &buf_info)
}

struct FrameBufferInfo {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    screencopy: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    outputs: Vec<wl_output::WlOutput>,
    layer_configure_serial: Option<u32>,
    click: Option<(i32, i32)>,
    pointer_x: f64,
    pointer_y: f64,
    frame_buffer: Option<FrameBufferInfo>,
    screencopy_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    frame_ready: bool,
    frame_failed: bool,
}

impl State {
    fn new() -> Self {
        Self {
            compositor: None,
            shm: None,
            seat: None,
            layer_shell: None,
            screencopy: None,
            outputs: Vec::new(),
            layer_configure_serial: None,
            click: None,
            pointer_x: 0.0,
            pointer_y: 0.0,
            frame_buffer: None,
            screencopy_frame: None,
            frame_ready: false,
            frame_failed: false,
        }
    }
}

// --- Dispatch implementations ---

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind(name, version.min(9), qh, ()));
                }
                "wl_output" => {
                    state.outputs.push(registry.bind(name, version.min(4), qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "zwlr_screencopy_manager_v1" => {
                    state.screencopy = Some(registry.bind(name, version.min(3), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
            }
            wl_pointer::Event::Button { state: btn_state, .. } => {
                if btn_state == WEnum::Value(wl_pointer::ButtonState::Pressed) {
                    state.click = Some((state.pointer_x as i32, state.pointer_y as i32));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            state.layer_configure_serial = Some(serial);
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer { format, width, height, stride } => {
                if let WEnum::Value(fmt) = format {
                    state.frame_buffer = Some(FrameBufferInfo {
                        format: fmt,
                        width,
                        height,
                        stride,
                    });
                    state.screencopy_frame = Some(proxy.clone());
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                state.frame_ready = true;
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                state.frame_failed = true;
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

fn create_shm_file(size: usize) -> Result<std::fs::File, PixlError> {
    use std::ffi::CString;
    use std::os::unix::io::FromRawFd;

    let name = CString::new(format!("/pixl-shm-{}", std::process::id()))
        .map_err(|e| PixlError::WaylandError(e.to_string()))?;

    // SAFETY: shm_open with valid C string, we check the result
    let fd = unsafe {
        libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR | libc::O_EXCL, 0o600)
    };
    if fd < 0 {
        return Err(PixlError::WaylandError(format!(
            "shm_open failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    // Unlink immediately so it's cleaned up
    unsafe { libc::shm_unlink(name.as_ptr()) };

    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.set_len(size as u64)
        .map_err(|e| PixlError::WaylandError(format!("ftruncate: {}", e)))?;

    Ok(file)
}

fn read_pixel_from_shm(file: &std::fs::File, info: &FrameBufferInfo) -> Result<RgbColour, PixlError> {
    use std::io::{Read, Seek};

    let size = (info.stride * info.height) as usize;
    let mut buf = vec![0u8; size];

    let mut f = file;
    f.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;
    f.read_exact(&mut buf)
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;

    match info.format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 => {
            if buf.len() < 4 {
                return Err(PixlError::CaptureError("buffer too small".into()));
            }
            // ARGB8888 in little-endian: B, G, R, A
            Ok(RgbColour { b: buf[0], g: buf[1], r: buf[2] })
        }
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => {
            if buf.len() < 4 {
                return Err(PixlError::CaptureError("buffer too small".into()));
            }
            Ok(RgbColour { r: buf[0], g: buf[1], b: buf[2] })
        }
        other => Err(PixlError::CaptureError(format!(
            "unsupported buffer format: {:?}",
            other
        ))),
    }
}
