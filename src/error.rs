//! Unified error type for pixl.
//!
//! All fallible functions in the crate return `Result<T, PixlError>`.
//! Errors carry actionable messages so the user knows what went wrong
//! and how to fix it.

#[derive(Debug, thiserror::Error)]
pub enum PixlError {
    #[error("No display server detected. Neither $WAYLAND_DISPLAY nor $DISPLAY is set.")]
    NoDisplayServer,

    #[error("Wayland compositor does not support wlr-screencopy protocol")]
    ProtocolUnsupported,

    #[error("xdg-desktop-portal colour picker unavailable: {0}")]
    PortalUnavailable(String),

    #[error("Wayland connection error: {0}")]
    WaylandError(String),

    #[error("X11 connection error: {0}")]
    X11Error(String),

    #[error("Failed to capture pixel: {0}")]
    CaptureError(String),

    #[error("Colour conversion error: {0}")]
    ColourError(String),
}
