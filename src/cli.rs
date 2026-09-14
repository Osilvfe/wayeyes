use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BackendKind {
    /// Pick the best available backend. Currently falls back to local pointer events.
    Auto,
    /// Track the pointer only while it is over the WayEyes surface.
    Local,
}

#[derive(Debug, Clone, Parser)]
#[command(name = "wayeyes", version, about = "xeyes reimagined for Wayland")]
pub struct Cli {
    /// Initial window width in logical pixels.
    #[arg(long, default_value_t = 260)]
    pub width: i32,

    /// Initial window height in logical pixels.
    #[arg(long, default_value_t = 150)]
    pub height: i32,

    /// Start without server-side/client-side window decorations.
    #[arg(long)]
    pub undecorated: bool,

    /// Pointer tracking backend.
    #[arg(long, value_enum, default_value_t = BackendKind::Auto)]
    pub backend: BackendKind,
}
