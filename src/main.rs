//! pixl — a minimal pixel colour picker for Linux.
//!
//! Grabs the colour of any pixel on screen by clicking it and prints
//! the value to stdout. Supports Wayland (wlroots + xdg-desktop-portal)
//! and X11.

mod colour;
mod error;
mod magnifier;
mod portal;
mod wayland;
mod x11;

use clap::Parser;
use colour::RgbColour;
use error::PixlError;
use std::env;
use std::process;

#[derive(Parser)]
#[command(
    name = "pixl",
    version,
    about = "Grab a pixel colour and print it to stdout"
)]
struct Cli {
    /// Output format: hex | rgb | hsl | hsv
    #[arg(short, long, default_value = "hex")]
    format: String,

    /// Disable the zoom magnifier lens
    #[arg(long)]
    no_magnifier: bool,
}

fn detect_and_pick(magnifier: bool) -> Result<RgbColour, PixlError> {
    let wayland_display = env::var("WAYLAND_DISPLAY").ok();
    let x_display = env::var("DISPLAY").ok();

    if wayland_display.is_some() {
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let is_wlroots = desktop
            .split(':')
            .any(|d| matches!(d, "Hyprland" | "sway" | "Sway" | "wlroots"));

        if is_wlroots {
            wayland::pick_colour(magnifier)
        } else {
            match wayland::pick_colour(magnifier) {
                Ok(c) => Ok(c),
                Err(PixlError::ProtocolUnsupported) => portal::pick_colour(),
                Err(e) => Err(e),
            }
        }
    } else if x_display.is_some() {
        x11::pick_colour(magnifier)
    } else {
        Err(PixlError::NoDisplayServer)
    }
}

fn format_colour(colour: &RgbColour, format: &str) -> Result<String, PixlError> {
    match format {
        "hex" => Ok(colour.to_hex()),
        "rgb" => Ok(colour.to_rgb_string()),
        "hsl" => Ok(colour.to_hsl_string()),
        "hsv" => Ok(colour.to_hsv_string()),
        other => Err(PixlError::ColourError(format!(
            "unknown format '{}'. Expected: hex, rgb, hsl, or hsv",
            other
        ))),
    }
}

fn run() -> Result<(), PixlError> {
    let cli = Cli::parse();
    let magnifier = !cli.no_magnifier;
    let colour = detect_and_pick(magnifier)?;
    let output = format_colour(&colour, &cli.format)?;
    println!("{}", output);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("pixl: {}", e);
        process::exit(1);
    }
}
