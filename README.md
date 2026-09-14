# WayEyes 👀

`xeyes`, reimagined for Wayland.

WayEyes is a Rust desktop toy and protocol experiment: two eyes that follow the pointer while respecting Wayland's security model. It uses a small GTK4/Cairo frontend and keeps pointer tracking behind replaceable backends.

## Status

### Milestone 0.3 — direct Wayland cursor tracking

- [x] Rust project
- [x] GTK4 native window
- [x] Cairo eye rendering with a pure-black default background
- [x] Local Wayland pointer tracking
- [x] Geometry and global/local coordinate calibration
- [x] Pure Wayland `ext-image-copy-capture-v1` cursor backend
- [x] `xdg-output` logical output origin mapping
- [x] ScreenCast Portal capability probing
- [x] ScreenCast session with `CursorMode::Metadata`
- [x] PipeWire `SPA_META_Cursor` reader
- [x] Multi-monitor portal stream origin/scale mapping
- [x] Backend events bridged back to the GTK main thread
- [x] Automatic backend order: direct Wayland → Portal → local
- [x] Unit tests and CI
- [ ] Harden direct-backend transform/fractional-scale mapping across more compositors
- [ ] Persisted portal sessions where supported
- [ ] Additional compositor-specific fallbacks where useful
- [ ] Layer-shell mode

On compositors implementing `ext-image-copy-capture-v1`, WayEyes can receive pointer-cursor position events directly from Wayland without capturing screen frames or starting PipeWire. Hyprland is the first target for this path. Its permission system may ask for the `cursorpos` permission before position events are delivered.

The ScreenCast Portal + PipeWire metadata backend remains available as a fallback on desktops that expose `CursorMode::Metadata`. Surface-local GTK pointer events are always useful because they pair a local sample with a global sample and calibrate the WayEyes surface origin.

## How global tracking works

The preferred backend uses the direct Wayland capture protocols:

```text
wl_output
    │
    ├── ext_output_image_capture_source_v1
    │              │
    │              ▼
    │   ext_image_copy_capture_cursor_session_v1
    │              │
    │        enter / leave / position
    │              │
    └── xdg-output logical origin
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

This cursor-only path does not request image frames. The current coordinate conversion is intentionally focused on Hyprland first; output transforms and fractional-scale behavior will be hardened as WayEyes is tested on more compositors.

When direct cursor capture is unavailable, WayEyes can fall back to the ScreenCast Portal path:

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
   + portal stream position/size
          │
          ▼
 global compositor coordinate
```

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

Force the direct Wayland cursor backend:

```bash
cargo run --release -- --backend wayland
```

Force the ScreenCast Portal + PipeWire backend:

```bash
cargo run --release -- --backend portal
```

Use only surface-local pointer events:

```bash
cargo run --release -- --backend local
```

For a small undecorated widget-like window:

```bash
cargo run --release -- --undecorated --width 260 --height 150
```

WayEyes currently defaults to GTK's Cairo renderer because some Wayland GPU-renderer stacks display `GtkDrawingArea` content as a black frame. Renderer selection can be overridden explicitly:

```bash
cargo run --release -- --renderer auto
cargo run --release -- --renderer gl
cargo run --release -- --renderer vulkan
cargo run --release -- --renderer cairo
```

Enable detailed logs with:

```bash
RUST_LOG=wayeyes=debug cargo run --release -- --backend wayland
```

With the direct backend, move the pointer over the WayEyes window once after startup to give the coordinate model a fresh local/global calibration sample. After that, the pupils should continue following the pointer when it leaves the window.

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
    ├── ext-image-copy-capture-v1 + xdg-output
    ├── XDG ScreenCast Portal + PipeWire metadata
    └── local Wayland/GTK pointer events
```

The renderer and coordinate model do not depend on the global pointer source. This is intentional: future backends can feed the same `BackendEvent::Pointer` path without changing the eye rendering code.

## License

MIT
