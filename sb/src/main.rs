// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

use clap::Parser;

use crate::app::TuiApp;

mod app;
mod chart;
mod client;
mod historical;
mod render;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = TuiApp::new(args.socket_path);
        app.run().await
    })
}

#[derive(Parser)]
#[command(name = "sb", version, about = "SteadyBearing terminal dashboard")]
pub struct Args {
    #[arg(
        long,
        default_value_t = String::from(sb_protocol::types::DEFAULT_SOCKET_PATH),
        help = "Path to the sb-daemon Unix socket"
    )]
    pub socket_path: String,
}
