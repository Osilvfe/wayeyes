use std::{
    io::Cursor,
    mem,
    os::fd::OwnedFd,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::{Context, anyhow, bail};
use ashpd::desktop::{
    PersistMode,
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as ScreencastStream,
    },
};
use pipewire as pw;
use pw::{properties::properties, spa};
use tracing::{debug, info, warn};

use super::BackendEvent;
use crate::geometry::Point;

const BACKEND_NAME: &str = "portal";

pub fn spawn() -> Receiver<BackendEvent> {
    let (tx, rx) = mpsc::channel();

    thread::Builder::new()
        .name("wayeyes-portal".into())
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
        .expect("failed to spawn portal backend thread");

    rx
}

fn run(tx: Sender<BackendEvent>) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create portal runtime")?;

    let (streams, fd) = runtime.block_on(open_portal())?;
    drop(runtime);

    tx.send(BackendEvent::Ready {
        backend: BACKEND_NAME,
        streams: streams.len(),
    })
    .ok();

    run_pipewire(streams, fd, tx)
}

async fn open_portal() -> anyhow::Result<(Vec<ScreencastStream>, OwnedFd)> {
    let proxy = Screencast::new()
        .await
        .context("failed to connect to ScreenCast portal")?;

    let cursor_modes = proxy
        .available_cursor_modes()
        .await
        .context("failed to query portal cursor modes")?;
    if !cursor_modes.contains(CursorMode::Metadata) {
        bail!("ScreenCast portal does not advertise Metadata cursor mode");
    }

    let source_types = proxy
        .available_source_types()
        .await
        .context("failed to query portal source types")?;
    if !source_types.contains(SourceType::Monitor) {
        bail!("ScreenCast portal does not advertise monitor capture");
    }

    info!(backend = BACKEND_NAME, "requesting monitor cursor metadata");

    let session = proxy
        .create_session(Default::default())
        .await
        .context("failed to create ScreenCast session")?;

    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Metadata)
                .set_sources(SourceType::Monitor)
                .set_multiple(true)
                .set_restore_token(None)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .context("failed to configure ScreenCast sources")?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .context("failed to start ScreenCast portal request")?
        .response()
        .context("ScreenCast portal request was rejected")?;

    let streams = response.streams().to_vec();
    if streams.is_empty() {
        bail!("ScreenCast portal returned no streams");
    }

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .context("failed to open PipeWire remote")?;

    Ok((streams, fd))
}

struct StreamData {
    tx: Sender<BackendEvent>,
    origin: (i32, i32),
    compositor_size: Option<(i32, i32)>,
    video: spa::param::video::VideoInfoRaw,
}

impl StreamData {
    fn global_cursor(&self, cursor: spa::utils::Point) -> Option<Point> {
        let size = self.video.size();
        let width = i32::try_from(size.width).ok()?;
        let height = i32::try_from(size.height).ok()?;

        if width > 0
            && height > 0
            && (cursor.x < 0 || cursor.y < 0 || cursor.x >= width || cursor.y >= height)
        {
            return None;
        }

        let (scale_x, scale_y) = match self.compositor_size {
            Some((logical_width, logical_height)) if width > 0 && height > 0 => (
                logical_width as f64 / width as f64,
                logical_height as f64 / height as f64,
            ),
            _ => (1.0, 1.0),
        };

        Some(Point::new(
            self.origin.0 as f64 + cursor.x as f64 * scale_x,
            self.origin.1 as f64 + cursor.y as f64 * scale_y,
        ))
    }
}

fn run_pipewire(
    streams: Vec<ScreencastStream>,
    fd: OwnedFd,
    tx: Sender<BackendEvent>,
) -> anyhow::Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopBox::new(None).context("failed to create PipeWire loop")?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)
        .context("failed to create PipeWire context")?;
    let core = context
        .connect_fd(fd, None)
        .context("failed to connect to portal PipeWire remote")?;

    let mut pw_streams = Vec::with_capacity(streams.len());
    let mut listeners = Vec::with_capacity(streams.len());

    for descriptor in streams {
        let node_id = descriptor.pipe_wire_node_id();
        let origin = descriptor.position().unwrap_or((0, 0));
        let compositor_size = descriptor.size();

        debug!(
            backend = BACKEND_NAME,
            node_id,
            ?origin,
            ?compositor_size,
            "connecting ScreenCast stream"
        );

        let stream = pw::stream::StreamBox::new(
            &core,
            "wayeyes-cursor",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .context("failed to create PipeWire stream")?;

        let data = StreamData {
            tx: tx.clone(),
            origin,
            compositor_size,
            video: Default::default(),
        };

        let listener = stream
            .add_local_listener_with_user_data(data)
            .state_changed(|_, _, old, new| {
                debug!(backend = BACKEND_NAME, ?old, ?new, "PipeWire state changed");
            })
            .param_changed(|stream, data, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }

                let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
                else {
                    return;
                };
                if media_type != spa::param::format::MediaType::Video
                    || media_subtype != spa::param::format::MediaSubtype::Raw
                {
                    return;
                }

                if data.video.parse(param).is_err() {
                    return;
                }

                let size = data.video.size();
                debug!(
                    backend = BACKEND_NAME,
                    width = size.width,
                    height = size.height,
                    "negotiated ScreenCast format"
                );

                if let Err(error) = request_cursor_metadata(stream) {
                    warn!(backend = BACKEND_NAME, %error, "failed to request cursor metadata");
                }
            })
            .process(|stream, data| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let Some(cursor) = buffer.find_meta::<spa::buffer::meta::MetaCursor>() else {
                    return;
                };
                if !cursor.is_valid() {
                    return;
                }

                let Some(global) = data.global_cursor(cursor.position()) else {
                    return;
                };
                let _ = data.tx.send(BackendEvent::Pointer(global));
            })
            .register()
            .context("failed to register PipeWire listener")?;

        let values = format_param()?;
        let pod = spa::pod::Pod::from_bytes(&values).map_err(|_| anyhow!("invalid format pod"))?;
        let mut params = [pod];

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .with_context(|| format!("failed to connect PipeWire node {node_id}"))?;

        listeners.push(listener);
        pw_streams.push(stream);
    }

    mainloop.run();

    drop(listeners);
    drop(pw_streams);
    Ok(())
}

fn format_param() -> anyhow::Result<Vec<u8>> {
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::RGB,
            spa::param::video::VideoFormat::RGB,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::YUY2,
            spa::param::video::VideoFormat::I420,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 16384,
                height: 16384
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );

    let serialized = spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(|error| anyhow!("failed to serialize format pod: {error}"))?;

    Ok(serialized.0.into_inner())
}

fn request_cursor_metadata(stream: &pw::stream::StreamRef) -> anyhow::Result<()> {
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            spa::pod::Property::new(
                spa::sys::SPA_PARAM_META_type,
                spa::pod::Value::Id(spa::utils::Id(spa::sys::SPA_META_Cursor)),
            ),
            spa::pod::Property::new(
                spa::sys::SPA_PARAM_META_size,
                spa::pod::Value::Int(mem::size_of::<spa::sys::spa_meta_cursor>() as i32),
            ),
        ],
    };

    let serialized = spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(|error| anyhow!("failed to serialize cursor metadata pod: {error}"))?;
    let values = serialized.0.into_inner();
    let pod = spa::pod::Pod::from_bytes(&values).map_err(|_| anyhow!("invalid metadata pod"))?;
    let mut params = [pod];
    stream
        .update_params(&mut params)
        .context("PipeWire rejected cursor metadata parameter")?;

    Ok(())
}
