# WayEyes 👀

`xeyes`, reimagined for Wayland.

WayEyes is a Rust desktop toy and protocol experiment: two eyes that follow the pointer while respecting Wayland's security model. The long-term goal is compositor-agnostic global pointer tracking through XDG Desktop Portal + PipeWire cursor metadata, with optional compositor-specific fallbacks where useful.

## Status

### Milestone 0.1 — foundation

- [x] Rust project
- [x] GTK4 native window
- [x] Cairo eye rendering
- [x] Local pointer tracking
- [x] Geometry/calibration model prepared for global coordinates
- [x] Unit tests and CI
- [ ] ScreenCast Portal capability probing
- [ ] PipeWire `SPA_META_Cursor` reader
- [ ] Portal/local coordinate calibration
- [ ] Persisted portal sessions where supported
- [ ] Optional native compositor backends
- [ ] Layer-shell mode

The current implementation follows the pointer only while it is over the WayEyes surface. This is intentional for the first milestone; global tracking is the next step.

## Why a portal backend?

Wayland core protocols do not expose unrestricted global pointer coordinates or global surface positions to ordinary clients. WayEyes therefore plans to request a monitor ScreenCast stream with cursor mode set to metadata and consume the cursor position from PipeWire metadata. When the pointer enters the WayEyes surface, local Wayland coordinates can be paired with the global cursor sample to calibrate the surface origin.

## Build

On Arch Linux:

```bash
sudo pacman -S --needed rust gtk4 pkgconf
cargo build
```

Run it with:

```bash
cargo run --release
```

For a small undecorated widget-like window:

```bash
cargo run --release -- --undecorated --width 260 --height 150
```

Enable logs with:

```bash
RUST_LOG=wayeyes=debug cargo run
```

## Architecture

```text
WayEyes
├── UI / renderer (GTK4 + Cairo)
├── CursorModel
│   ├── local pointer coordinate
│   ├── global pointer coordinate
│   └── calibrated surface origin
└── pointer backend
    ├── local Wayland events (implemented)
    ├── XDG ScreenCast Portal + PipeWire metadata (next)
    └── compositor-specific optional backends (future)
```

## License

MIT
