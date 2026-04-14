# pixl

Grab a pixel colour on X11 or Wayland. Print hex to stdout.

pixl is a minimal command-line pixel colour picker for Linux — the direct
successor to [grabc](https://github.com/muquit/grabc) for the Wayland era.

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
the desktop's native colour picker.

For X11 sessions, pixl grabs the pointer and reads the pixel value directly,
matching the behaviour of grabc.

## License

AGPL-3.0-or-later
