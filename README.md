# WayEyes 👀

`xeyes`, reimagined for Wayland.

WayEyes is a Rust desktop toy and protocol experiment: two eyes that follow the pointer while respecting Wayland's security model. It uses a small GTK4/Cairo frontend and keeps pointer tracking behind replaceable backends.

## Status

### Milestone 0.2 — global pointer backend

- [x] Rust project
- [x] GTK4 native window
- [x] Cairo eye rendering
- [x] Local Wayland pointer tracking
- [x] Geometry and global/local coordinate calibration
- [x] ScreenCast Portal capability probing
- [x] ScreenCast session with `CursorMode::Metadata`
- [x] PipeWire `SPA_META_Cursor` reader
- [x] Multi-monitor stream origin/scale mapping
- [x] Portal events bridged back to the GTK main thread
- [x] Unit tests and CI
- [ ] Pure Wayland `ext-image-copy-capture-v1` cursor backend
- [ ] Persisted portal sessions where supported
- [ ] Additional compositor-specific fallbacks where useful
- [ ] Layer-shell mode

When the selected portal backend supports cursor metadata, WayEyes can continue tracking the pointer after it leaves the WayEyes surface. The local Wayland pointer event is still useful: it pairs a surface-local sample with the global sample and calibrates the WayEyes surface origin.

If the portal does not advertise metadata cursor mode, `--backend auto` currently keeps the local tracker working. A direct Wayland cursor backend based on `ext-image-copy-capture-v1` is the next compatibility target.

## How global tracking works

Wayland core protocols intentionally do not expose unrestricted global pointer coordinates or global surface positions to ordinary clients. The current generic backend therefore uses the desktop ScreenCast portal and requests:

```text
ScreenCast Portal
    cursor_mode = Metadata
    source       = Monitor
          │
          ▼
      PipeWire
          │
          ▼
   SPA_META_Cursor
          │
          ▼
 stream cursor position
          │
   + portal stream position/size
          │
          ▼
 global compositor coordinate
          │
          ▼
      CursorModel
          │
          ▼
         👀
```

Portal stream coordinates can be in video pixels while the compositor reports logical stream geometry. WayEyes maps the negotiated PipeWire video size back into the compositor coordinate space before sending the point to `CursorModel`.

When the pointer is over the WayEyes surface:

```text
surface_origin = global_cursor - local_cursor
```

After that calibration, global samples can be converted back into WayEyes-local coordinates even while the pointer is over another client.

## Build

On Arch Linux:

```bash
sudo pacman -S --needed rust gtk4 pipewire pkgconf
cargo build
```

Run with automatic backend selection:

```bash
cargo run --release
```

Force the ScreenCast Portal + PipeWire backend:

```bash
cargo run --release -- --backend portal
```

Use only surface-local Wayland pointer events, without requesting portal access:

```bash
cargo run --release -- --backend local
```

For a small undecorated widget-like window:

```bash
cargo run --release -- --undecorated --width 260 --height 150
```

Enable logs with:

```bash
RUST_LOG=wayeyes=debug cargo run
```

The portal backend may present a monitor-sharing chooser. Select the monitor(s) that WayEyes should observe. Compositor/portal implementations that do not expose `Metadata` cursor mode will currently fall back to local tracking in automatic mode.

## Architecture

```text
WayEyes
├── UI / renderer
│   ├── GTK4
│   └── Cairo
├── CursorModel
│   ├── local pointer coordinate
│   ├── global pointer coordinate
│   └── calibrated surface origin
└── pointer backends
    ├── local Wayland events
    ├── XDG ScreenCast Portal + PipeWire metadata
    └── direct Wayland/compositor fallbacks (next)
```

The renderer and coordinate model do not depend on the global pointer source. This is intentional: future backends can feed the same `BackendEvent::Pointer` path without changing the eye rendering code.

## License

MIT
