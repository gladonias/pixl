//! wlroots screencopy capture path.
//!
//! Uses `zwlr_layer_shell_v1` for an overlay surface to capture pointer clicks,
//! and `zwlr_screencopy_manager_v1` to capture a 1×1 pixel at the clicked position.
//! Magnifier is TODO — continuous screencopy would exceed the line budget.

use crate::colour::RgbColour;
use crate::error::PixlError;
use std::os::unix::io::AsFd;
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

pub fn pick_colour(_magnifier: bool) -> Result<RgbColour, PixlError> {
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

    // Phase 1: overlay surface to capture click
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

    if let Some(serial) = st.configure_serial.take() {
        ls.ack_configure(serial);
        let fd = create_shm_file(4)?;
        let pool = shm.create_pool(fd.as_fd(), 4, &qh, ());
        let buf = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ());
        surface.attach(Some(&buf), 0, 0);
        surface.commit();
    }

    let _ptr = seat.get_pointer(&qh, ());
    loop {
        queue.blocking_dispatch(&mut st).map_err(wl_err)?;
        if st.click.is_some() {
            break;
        }
    }
    ls.destroy();
    surface.destroy();

    let (cx, cy) = st
        .click
        .ok_or_else(|| PixlError::CaptureError("no click".into()))?;

    // Phase 2: screencopy 1×1 pixel
    let scm = st.screencopy.clone().unwrap();
    let output = st.outputs[0].clone();
    let _frame = scm.capture_output_region(0, &output, cx, cy, 1, 1, &qh, ());
    queue.roundtrip(&mut st).map_err(wl_err)?;

    let fi = st
        .frame_buf
        .take()
        .ok_or_else(|| PixlError::CaptureError("no buffer info from screencopy".into()))?;
    let sz = (fi.stride * fi.height) as i32;
    let fd = create_shm_file(sz as usize)?;
    let pool = shm.create_pool(fd.as_fd(), sz, &qh, ());
    let buffer = pool.create_buffer(
        0,
        fi.width as i32,
        fi.height as i32,
        fi.stride as i32,
        fi.format,
        &qh,
        (),
    );
    if let Some(ref f) = st.sc_frame {
        f.copy(&buffer);
    }
    loop {
        queue.blocking_dispatch(&mut st).map_err(wl_err)?;
        if st.frame_ready || st.frame_failed {
            break;
        }
    }
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
    click: Option<(i32, i32)>,
    ptr_x: f64,
    ptr_y: f64,
    frame_buf: Option<BufInfo>,
    sc_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    frame_ready: bool,
    frame_failed: bool,
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
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            st.configure_serial = Some(serial);
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

fn create_shm_file(size: usize) -> Result<std::fs::File, PixlError> {
    use std::ffi::CString;
    use std::os::unix::io::FromRawFd;

    let name = CString::new(format!("/pixl-shm-{}", std::process::id())).map_err(wl_err)?;
    // SAFETY: shm_open with valid C string, result checked
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

fn read_pixel(file: &std::fs::File, info: &BufInfo) -> Result<RgbColour, PixlError> {
    use std::io::{Read, Seek};
    let mut buf = vec![0u8; (info.stride * info.height) as usize];
    let mut f = file;
    f.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;
    f.read_exact(&mut buf)
        .map_err(|e| PixlError::CaptureError(e.to_string()))?;
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
