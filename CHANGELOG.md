# Changelog

All notable changes to pixl are documented in this file.

## v0.2.1 — 2026-04-15

- Fix unbounded D-Bus event loop in the portal backend — now times out after 120 s instead of blocking forever
- Fix case-sensitive desktop detection — `XDG_CURRENT_DESKTOP=hyprland` (lowercase) is now correctly routed to the wlroots backend
- Clamp portal colour channel values to 0.0–1.0 before conversion to prevent silent truncation on out-of-range responses

## v0.2.0 — 2026-04-15

- Magnifier lens for X11 and wlroots Wayland — zooms in around the cursor with a 15×15 pixel grid and crosshair overlay
- Full-screen snapshot capture at startup (screencopy on Wayland, GetImage on X11) used for both magnifier rendering and pixel readback
- Magnifier window repositions to avoid screen edges
- `--no-magnifier` now fully functional on both backends (previously was a no-op stub)

## v0.1.0 — 2026-04-15

- Pixel colour picking on X11 via `GetImage` with crosshair cursor
- Wayland support for wlroots compositors (Hyprland, Sway) via `zwlr_screencopy_manager_v1`
- Fallback to `org.freedesktop.portal.Screenshot.PickColor` over D-Bus for non-wlroots compositors (KDE Plasma, GNOME)
- Four output formats: hex (default), rgb, hsl, hsv
- 17 unit tests for colour conversions
