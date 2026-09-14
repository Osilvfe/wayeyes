use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BackendKind {
    /// Pick the best available backend, preferring the ScreenCast portal.
    Auto,
    /// Use XDG Desktop Portal + PipeWire cursor metadata.
    Portal,
    /// Track the pointer only while it is over the WayEyes surface.
    Local,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RendererKind {
    /// Let GTK select its normal platform renderer.
    Auto,
    /// Use GTK's Cairo fallback renderer.
    Cairo,
    /// Use GTK's OpenGL renderer.
    Gl,
    /// Use GTK's Vulkan renderer.
    Vulkan,
}

impl RendererKind {
    pub const fn gsk_name(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Cairo => Some("cairo"),
            Self::Gl => Some("gl"),
            Self::Vulkan => Some("vulkan"),
        }
    }
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

    /// GSK renderer. If omitted, WayEyes uses cairo unless GSK_RENDERER is already set.
    #[arg(long, value_enum)]
    pub renderer: Option<RendererKind>,
}
