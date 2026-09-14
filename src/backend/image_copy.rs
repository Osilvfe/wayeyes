use std::{
    collections::BTreeMap,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::{Context, bail};
use tracing::{debug, info, warn};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop,
    protocol::{wl_output, wl_pointer, wl_registry, wl_seat},
};
use wayland_protocols::{
    ext::{
        image_capture_source::v1::client::{
            ext_image_capture_source_v1::ExtImageCaptureSourceV1,
            ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
        },
        image_copy_capture::v1::client::{
            ext_image_copy_capture_cursor_session_v1::{self, ExtImageCopyCaptureCursorSessionV1},
            ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
        },
    },
    xdg::xdg_output::zv1::client::{
        zxdg_output_manager_v1::ZxdgOutputManagerV1,
        zxdg_output_v1::{self, ZxdgOutputV1},
    },
};

use super::BackendEvent;
use crate::geometry::Point;

const BACKEND_NAME: &str = "wayland";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OutputId(u32);

struct OutputState {
    output: wl_output::WlOutput,
    xdg_output: Option<ZxdgOutputV1>,
    source: Option<ExtImageCaptureSourceV1>,
    cursor_session: Option<ExtImageCopyCaptureCursorSessionV1>,
    wl_origin: Option<(i32, i32)>,
    logical_origin: Option<(i32, i32)>,
    logical_size: Option<(i32, i32)>,
}

impl OutputState {
    fn new(output: wl_output::WlOutput) -> Self {
        Self {
            output,
            xdg_output: None,
            source: None,
            cursor_session: None,
            wl_origin: None,
            logical_origin: None,
            logical_size: None,
        }
    }

    fn origin(&self) -> (i32, i32) {
        self.logical_origin.or(self.wl_origin).unwrap_or((0, 0))
    }
}

struct WaylandState {
    tx: Sender<BackendEvent>,
    seats: Vec<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    outputs: BTreeMap<OutputId, OutputState>,
    source_manager: Option<ExtOutputImageCaptureSourceManagerV1>,
    capture_manager: Option<ExtImageCopyCaptureManagerV1>,
    xdg_output_manager: Option<ZxdgOutputManagerV1>,
}

impl WaylandState {
    fn new(tx: Sender<BackendEvent>) -> Self {
        Self {
            tx,
            seats: Vec::new(),
            pointer: None,
            outputs: BTreeMap::new(),
            source_manager: None,
            capture_manager: None,
            xdg_output_manager: None,
        }
    }

    fn validate_globals(&self) -> anyhow::Result<()> {
        if self.capture_manager.is_none() {
            bail!("compositor does not advertise ext_image_copy_capture_manager_v1");
        }
        if self.source_manager.is_none() {
            bail!("compositor does not advertise ext_output_image_capture_source_manager_v1");
        }
        if self.seats.is_empty() {
            bail!("compositor did not advertise a wl_seat");
        }
        if self.outputs.is_empty() {
            bail!("compositor did not advertise any wl_output");
        }
        Ok(())
    }

    fn prepare_outputs(&mut self, qh: &QueueHandle<Self>) {
        let source_manager = self.source_manager.clone();
        let xdg_output_manager = self.xdg_output_manager.clone();

        for (id, output) in &mut self.outputs {
            if output.source.is_none() {
                if let Some(manager) = source_manager.as_ref() {
                    output.source = Some(manager.create_source(&output.output, qh, ()));
                }
            }

            if output.xdg_output.is_none() {
                if let Some(manager) = xdg_output_manager.as_ref() {
                    output.xdg_output = Some(manager.get_xdg_output(&output.output, qh, *id));
                }
            }
        }
    }

    fn start_cursor_sessions(&mut self, qh: &QueueHandle<Self>) -> anyhow::Result<usize> {
        let pointer = self
            .pointer
            .clone()
            .context("wl_seat does not expose a pointer capability")?;
        let manager = self
            .capture_manager
            .clone()
            .context("ext_image_copy_capture_manager_v1 disappeared")?;

        let mut count = 0;
        for (id, output) in &mut self.outputs {
            if output.cursor_session.is_some() {
                count += 1;
                continue;
            }

            let Some(source) = output.source.as_ref() else {
                continue;
            };

            output.cursor_session =
                Some(manager.create_pointer_cursor_session(source, &pointer, qh, *id));
            count += 1;
        }

        if count == 0 {
            bail!("no output cursor sessions could be created");
        }
        Ok(count)
    }

    fn send_pointer(&self, id: OutputId, x: i32, y: i32) {
        let Some(output) = self.outputs.get(&id) else {
            return;
        };
        let (origin_x, origin_y) = output.origin();
        let global = Point::new(f64::from(origin_x + x), f64::from(origin_y + y));
        let _ = self.tx.send(BackendEvent::Pointer(global));
    }
}

pub fn spawn() -> Receiver<BackendEvent> {
    let (tx, rx) = mpsc::channel();

    thread::Builder::new()
        .name("wayeyes-wayland".into())
        .spawn(move || {
            if let Err(error) = run(tx.clone()) {
                let message = format!("{error:#}");
                warn!(backend = BACKEND_NAME, %message, "backend stopped");
                let _ = tx.send(BackendEvent::Failed {
                    backend: BACKEND_NAME,
                    message,
                });
            }
        })
        .expect("failed to spawn direct Wayland backend thread");

    rx
}

fn run(tx: Sender<BackendEvent>) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env().context("failed to connect to Wayland compositor")?;
    let display = conn.display();
    let mut queue = conn.new_event_queue::<WaylandState>();
    let qh = queue.handle();
    let _registry = display.get_registry(&qh, ());
    let mut state = WaylandState::new(tx.clone());

    // First roundtrip discovers and binds globals.
    queue
        .roundtrip(&mut state)
        .context("failed to enumerate Wayland globals")?;
    state.validate_globals()?;

    // Create output capture sources and xdg-output metadata objects. Seat
    // capabilities from the bindings above are delivered on the next roundtrip.
    state.prepare_outputs(&qh);
    queue
        .roundtrip(&mut state)
        .context("failed to initialize Wayland output metadata")?;

    let streams = state.start_cursor_sessions(&qh)?;
    tx.send(BackendEvent::Ready {
        backend: BACKEND_NAME,
        streams,
    })
    .ok();

    info!(
        backend = BACKEND_NAME,
        streams,
        "direct Wayland cursor capture ready"
    );

    loop {
        queue
            .blocking_dispatch(&mut state)
            .context("Wayland cursor event dispatch failed")?;
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_seat" => {
                    let seat =
                        registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(9), qh, ());
                    state.seats.push(seat);
                }
                "wl_output" => {
                    let id = OutputId(name);
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, id);
                    state.outputs.insert(id, OutputState::new(output));
                }
                "ext_output_image_capture_source_manager_v1" => {
                    state.source_manager =
                        Some(registry.bind::<ExtOutputImageCaptureSourceManagerV1, _, _>(
                            name,
                            1,
                            qh,
                            (),
                        ));
                }
                "ext_image_copy_capture_manager_v1" => {
                    state.capture_manager =
                        Some(registry.bind::<ExtImageCopyCaptureManagerV1, _, _>(name, 1, qh, ()));
                }
                "zxdg_output_manager_v1" => {
                    state.xdg_output_manager = Some(registry.bind::<ZxdgOutputManagerV1, _, _>(
                        name,
                        version.min(3),
                        qh,
                        (),
                    ));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.remove(&OutputId(name));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
                debug!(backend = BACKEND_NAME, "bound wl_pointer");
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, OutputId> for WaylandState {
    fn event(
        state: &mut Self,
        _output: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &OutputId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(id) else {
            return;
        };

        if let wl_output::Event::Geometry { x, y, .. } = event {
            output.wl_origin = Some((x, y));
        }
    }
}

impl Dispatch<ZxdgOutputV1, OutputId> for WaylandState {
    fn event(
        state: &mut Self,
        _xdg_output: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        id: &OutputId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(id) else {
            return;
        };

        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical_origin = Some((x, y));
                debug!(
                    backend = BACKEND_NAME,
                    output = id.0,
                    x,
                    y,
                    "xdg-output logical origin"
                );
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_size = Some((width, height));
                debug!(
                    backend = BACKEND_NAME,
                    output = id.0,
                    width,
                    height,
                    "xdg-output logical size"
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, OutputId> for WaylandState {
    fn event(
        state: &mut Self,
        _session: &ExtImageCopyCaptureCursorSessionV1,
        event: ext_image_copy_capture_cursor_session_v1::Event,
        id: &OutputId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_image_copy_capture_cursor_session_v1::Event::Enter => {
                debug!(
                    backend = BACKEND_NAME,
                    output = id.0,
                    "cursor entered capture source"
                );
            }
            ext_image_copy_capture_cursor_session_v1::Event::Leave => {
                debug!(
                    backend = BACKEND_NAME,
                    output = id.0,
                    "cursor left capture source"
                );
            }
            ext_image_copy_capture_cursor_session_v1::Event::Position { x, y } => {
                state.send_pointer(*id, x, y);
            }
            ext_image_copy_capture_cursor_session_v1::Event::Hotspot { .. } => {}
            _ => {}
        }
    }
}

delegate_noop!(WaylandState: ignore wl_pointer::WlPointer);
delegate_noop!(WaylandState: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(WaylandState: ExtImageCaptureSourceV1);
delegate_noop!(WaylandState: ExtImageCopyCaptureManagerV1);
delegate_noop!(WaylandState: ZxdgOutputManagerV1);
