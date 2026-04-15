# pixl

Grab a pixel colour on X11 or Wayland. Print hex to stdout.

pixl is a minimal command-line pixel colour picker for Linux — the direct
successor to [grabc](https://github.com/muquit/grabc) for the Wayland era.
Click any pixel on screen and pixl prints its colour value to stdout, ready
to pipe into clipboard managers, scripts, or other tools.

## Design

pixl follows the Unix philosophy:

- **One job** — grab a pixel colour, print it, exit.
- **Zero runtime dependencies** — single compiled binary, nothing to install.
- **Composable** — output goes to stdout; the shell handles the rest.
- **No GUI, no config, no daemon** — just a tool.
- **Under 800 lines of Rust** across all source files.

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

On Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libwayland-dev wayland-protocols libxkbcommon-dev libdbus-1-dev libx11-dev
```

On Fedora:

```bash
sudo dnf install gcc pkg-config wayland-devel wayland-protocols-devel libxkbcommon-devel dbus-devel libX11-devel
```

## Usage

Run `pixl` and click anywhere on screen. The colour of the clicked pixel is
printed to stdout and the program exits.

```bash
pixl
#404552
```

### Output formats

Use `-f` / `--format` to choose the output format:

```bash
pixl                # hex (default): #404552
pixl -f hex         # hex:           #404552
pixl -f rgb         # rgb:           rgb(64, 69, 82)
pixl -f hsl         # hsl:           hsl(222, 12%, 29%)
pixl -f hsv         # hsv:           hsv(222, 22%, 32%)
```

### Piping to clipboard

```bash
# Wayland (requires wl-copy from wl-clipboard)
pixl | wl-copy

# X11 (requires xclip)
pixl | xclip -selection clipboard

# X11 (requires xsel)
pixl | xsel --clipboard
```

### Scripting examples

```bash
# Store the picked colour in a variable
colour=$(pixl)
echo "You picked: $colour"

# Set a terminal colour
printf '\033]11;%s\007' "$(pixl)"

# Notify with the picked colour
notify-send "Colour" "$(pixl)"

# Pick and convert to uppercase hex
pixl | tr '[:lower:]' '[:upper:]'
```

### Options

```
pixl [OPTIONS]

Options:
  -f, --format <FORMAT>  Output format: hex | rgb | hsl | hsv  [default: hex]
      --no-magnifier     Disable the zoom magnifier lens
  -V, --version          Print version and exit
  -h, --help             Print help and exit
```

Exit code is 0 on success, non-zero on any error. Errors are printed to
stderr.

## How it works

pixl automatically detects the display server and compositor at startup:

1. If `$WAYLAND_DISPLAY` is set, it's a Wayland session. pixl checks
   `$XDG_CURRENT_DESKTOP` to determine the compositor:
   - **wlroots compositors** (Hyprland, Sway): uses the
     `zwlr_screencopy_manager_v1` Wayland protocol to capture the pixel
     directly. A transparent overlay surface intercepts the click.
   - **Other compositors** (KDE Plasma, GNOME): tries the wlroots path first;
     if the protocol isn't available, falls back to calling
     `org.freedesktop.portal.Screenshot.PickColor` over D-Bus, which opens the
     desktop's native colour picker.
2. If only `$DISPLAY` is set, it's an X11 session. pixl grabs the pointer with
   a crosshair cursor and uses `GetImage` to read the pixel at the clicked
   position — the same approach as grabc.
3. If neither variable is set, pixl exits with a clear error message.

## Compositor compatibility

| Compositor       | Session   | Capture method              | Status    |
|------------------|-----------|-----------------------------|-----------|
| Hyprland         | Wayland   | `zwlr_screencopy_manager_v1`| Supported |
| Sway             | Wayland   | `zwlr_screencopy_manager_v1`| Supported |
| Other wlroots WMs| Wayland   | `zwlr_screencopy_manager_v1`| Supported |
| KDE Plasma       | Wayland   | xdg-desktop-portal          | Supported |
| GNOME            | Wayland   | xdg-desktop-portal          | Supported |
| X11 / Xorg       | X11       | `GetImage` (x11rb)          | Supported |

### Notes

- On KDE Plasma and GNOME Wayland, pixl relies on `xdg-desktop-portal` and
  its backend (`xdg-desktop-portal-kde` or `xdg-desktop-portal-gnome`). These
  are typically installed by default with the desktop environment.
- The portal path opens the desktop's own colour picker UI — pixl does not draw
  its own overlay in this case.
- On wlroots compositors, pixl creates a fullscreen transparent overlay on the
  `zwlr_layer_shell_v1` overlay layer to intercept the click.

## Colour formats

| Format | Example output       | Description                        |
|--------|----------------------|------------------------------------|
| hex    | `#404552`            | Lowercase hex, 6 digits            |
| rgb    | `rgb(64, 69, 82)`    | CSS-style, 0-255 integer channels  |
| hsl    | `hsl(222, 12%, 29%)` | Hue 0-360, saturation/lightness %  |
| hsv    | `hsv(222, 22%, 32%)` | Hue 0-360, saturation/value %      |

## Project structure

```
src/
├── main.rs       CLI args, session detection, dispatch
├── wayland.rs    wlr-screencopy path (Hyprland, Sway)
├── portal.rs     xdg-desktop-portal path (KDE, GNOME)
├── x11.rs        X11/Xorg path via x11rb
├── colour.rs     RgbColour and format conversions
├── error.rs      PixlError enum (thiserror)
└── lib.rs        Public module re-exports for tests
tests/
└── colour_tests.rs   17 unit tests for colour conversions
```

## Changelog

### v0.1.0 — 2026-04-15

Initial release.

- Pixel colour picking on X11 via `GetImage` with crosshair cursor and magnifier overlay
- Wayland support for wlroots compositors (Hyprland, Sway) via `zwlr_screencopy_manager_v1` with transparent overlay click interception
- Fallback to `org.freedesktop.portal.Screenshot.PickColor` over D-Bus for non-wlroots Wayland compositors (KDE Plasma, GNOME)
- Automatic display server and compositor detection at startup
- Four output formats: hex (default), rgb, hsl, hsv
- `--no-magnifier` flag to disable the zoom lens on X11
- Single static binary, zero runtime dependencies
- 17 unit tests for colour conversions
- Under 800 lines of Rust

## License

AGPL-3.0-or-later
