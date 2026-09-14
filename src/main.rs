mod backend;
mod cli;
mod geometry;
mod ui;

use clap::Parser;
use cli::Cli;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("wayeyes=info")),
        )
        .init();

    let cli = Cli::parse();
    ui::run(cli)
}
