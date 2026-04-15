# pixl

Grab a pixel colour on X11 or Wayland. Print hex to stdout.

pixl is a minimal command-line pixel colour picker for Linux — the direct
successor to [grabc](https://github.com/muquit/grabc) for the Wayland era.

A built-in magnifier lens zooms in around the cursor so you can pick
individual pixels accurately on both X11 and Wayland (wlroots compositors).

## Installation

Build from source (requires Rust 1.70+):

```bash
git clone https://codeberg.org/gadonias/pixl.git
cd pixl
cargo build --release
cp target/release/pixl ~/.local/bin/
```

### Build dependencies

- Rust toolchain (rustc, cargo)
- pkg-config
- wayland, wayland-protocols
- libxkbcommon
- dbus
- libx11

On Arch Linux:

```bash
sudo pacman -S --needed base-devel pkg-config wayland wayland-protocols libxkbcommon dbus libx11
```

There are **no runtime dependencies** — pixl is a single statically-linked binary.

## Usage

```bash
# Pick a colour, print hex (default)
pixl
#404552

# Output as RGB
pixl -f rgb
rgb(64, 69, 82)

# Output as HSL
pixl -f hsl
hsl(222, 12%, 29%)

# Output as HSV
pixl -f hsv
hsv(222, 22%, 32%)

# Pipe to clipboard (wl-copy for Wayland, xclip for X11)
pixl | wl-copy
pixl | xclip -selection clipboard

# Use in scripts
BG=$(pixl)
echo "Selected colour: $BG"

# Disable the magnifier overlay
pixl --no-magnifier
```

## Magnifier

By default, pixl shows a zoomed-in lens that follows your cursor, rendering
a 15×15 pixel grid scaled up so you can pick individual pixels precisely.
The center pixel (highlighted with a red crosshair) is the one that gets
selected on click.

The magnifier works by capturing a full-screen snapshot once at startup,
then rendering the zoomed view from that buffer on each pointer motion.
This means it shows a static snapshot — animations or video won't update
in the lens, but for colour picking this is not an issue.

Disable it with `--no-magnifier` if you prefer a plain crosshair cursor.

### Magnifier support by compositor

| Compositor       | Magnifier | Notes |
|------------------|-----------|-------|
| X11 / Xorg       | Yes       | Overlay window with `OverrideRedirect` |
| Hyprland / Sway  | Yes       | Layer-shell overlay via `zwlr_layer_shell_v1` |
| KDE Plasma       | No        | Portal uses desktop's native picker (has its own zoom) |
| GNOME            | No        | Portal uses desktop's native picker |

## Compositor compatibility

| Compositor       | Session   | Path used         | Status |
|------------------|-----------|-------------------|--------|
| Hyprland         | Wayland   | wlr-screencopy    | Supported |
| Sway             | Wayland   | wlr-screencopy    | Supported |
| KDE Plasma       | Wayland   | xdg-desktop-portal| Supported |
| GNOME            | Wayland   | xdg-desktop-portal| Supported |
| X11 / Xorg       | X11       | XGetImage          | Supported |

For wlroots-based compositors (Hyprland, Sway), pixl uses the
`zwlr_screencopy_manager_v1` protocol to capture pixels directly.

For other Wayland compositors (KDE Plasma, GNOME), pixl falls back to the
`org.freedesktop.portal.Screenshot.PickColor` D-Bus interface, which invokes
the desktop's native colour picker (which has its own zoom/magnifier).

For X11 sessions, pixl grabs the pointer and reads the pixel value directly,
matching the behaviour of grabc.

## Changelog

### v0.2.0 — 2026-04-15

- Magnifier lens for X11 and wlroots Wayland — zooms in around the cursor with a 15×15 pixel grid and crosshair overlay
- Full-screen snapshot capture at startup (screencopy on Wayland, GetImage on X11) used for both magnifier rendering and pixel readback
- Magnifier window repositions to avoid screen edges
- `--no-magnifier` now fully functional on both backends (previously was a no-op stub)

### v0.1.0 — 2026-04-15

- Pixel colour picking on X11 via `GetImage` with crosshair cursor
- Wayland support for wlroots compositors (Hyprland, Sway) via `zwlr_screencopy_manager_v1`
- Fallback to `org.freedesktop.portal.Screenshot.PickColor` over D-Bus for non-wlroots compositors (KDE Plasma, GNOME)
- Four output formats: hex (default), rgb, hsl, hsv
- 17 unit tests for colour conversions

## License

AGPL-3.0-or-later
