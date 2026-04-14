//! xdg-desktop-portal colour picker path.
//!
//! Uses `org.freedesktop.portal.Screenshot.PickColor` over D-Bus.
//! Works on KDE Plasma Wayland and GNOME Wayland via their respective
//! portal implementations. The portal shows its own native picker UI,
//! so no magnifier or overlay is needed from our side.

use crate::colour::RgbColour;
use crate::error::PixlError;
use dbus::arg::RefArg;
use dbus::blocking::Connection;
use std::collections::HashMap;
use std::time::Duration;

pub fn pick_colour() -> Result<RgbColour, PixlError> {
    let conn = Connection::new_session().map_err(|e| {
        PixlError::PortalUnavailable(format!("D-Bus session connection failed: {}", e))
    })?;

    let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_secs(120),
    );

    let window_handle = "";
    let options: HashMap<&str, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>> = HashMap::new();

    // Call PickColor — this blocks until the user picks a colour via the
    // portal's native UI or cancels
    let (response_path,): (dbus::Path,) = proxy
        .method_call(
            "org.freedesktop.portal.Screenshot",
            "PickColor",
            (window_handle, options),
        )
        .map_err(|e| {
            PixlError::PortalUnavailable(format!(
                "PickColor call failed (is xdg-desktop-portal running?): {}",
                e
            ))
        })?;

    // Listen for the Response signal on the returned object path
    let response_proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        response_path,
        Duration::from_secs(120),
    );

    // Add match for the Response signal
    let (tx, rx) = std::sync::mpsc::channel::<Result<RgbColour, PixlError>>();

    let _signal_id = response_proxy
        .match_signal(
            move |resp: OrgFreedesktopPortalRequestResponse,
                  _conn: &Connection,
                  _msg: &dbus::Message| {
                let result = parse_pick_color_response(resp);
                let _ = tx.send(result);
                false // remove match after first signal
            },
        )
        .map_err(|e| {
            PixlError::PortalUnavailable(format!("failed to listen for portal response: {}", e))
        })?;

    // Process D-Bus messages until we get the response
    loop {
        conn.process(Duration::from_millis(200))
            .map_err(|e| PixlError::PortalUnavailable(format!("D-Bus processing error: {}", e)))?;

        if let Ok(result) = rx.try_recv() {
            return result;
        }
    }
}

fn parse_pick_color_response(
    resp: OrgFreedesktopPortalRequestResponse,
) -> Result<RgbColour, PixlError> {
    if resp.response != 0 {
        return Err(PixlError::PortalUnavailable(
            "user cancelled the colour picker".into(),
        ));
    }

    let colour = resp
        .results
        .get("color")
        .ok_or_else(|| PixlError::CaptureError("portal response missing 'color' key".into()))?;

    // The color value is a (ddd) — tuple of 3 f64 in linear RGB 0.0–1.0
    let iter = colour
        .0
        .as_iter()
        .ok_or_else(|| PixlError::CaptureError("colour value is not iterable".into()))?;

    let channels: Vec<f64> = iter.filter_map(|v: &dyn RefArg| v.as_f64()).collect();

    if channels.len() < 3 {
        return Err(PixlError::CaptureError(format!(
            "expected 3 colour channels, got {}",
            channels.len()
        )));
    }

    Ok(RgbColour {
        r: (channels[0] * 255.0).round() as u8,
        g: (channels[1] * 255.0).round() as u8,
        b: (channels[2] * 255.0).round() as u8,
    })
}

// D-Bus signal struct for org.freedesktop.portal.Request.Response
#[derive(Debug)]
struct OrgFreedesktopPortalRequestResponse {
    response: u32,
    results: HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg + 'static>>>,
}

impl dbus::arg::ReadAll for OrgFreedesktopPortalRequestResponse {
    fn read(i: &mut dbus::arg::Iter) -> Result<Self, dbus::arg::TypeMismatchError> {
        let response: u32 = i.read()?;
        let results: HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg + 'static>>> =
            i.read()?;
        Ok(Self { response, results })
    }
}

impl dbus::message::SignalArgs for OrgFreedesktopPortalRequestResponse {
    const NAME: &'static str = "Response";
    const INTERFACE: &'static str = "org.freedesktop.portal.Request";
}
