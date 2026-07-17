use clap::Parser;

use crate::app::TuiApp;

mod app;
mod chart;
mod client;
mod historical;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = TuiApp::new(args.socket_path);
        app.run().await
    })
}

#[derive(Parser)]
pub struct Args {
    #[arg(long, default_value = "/tmp/sb.sock")]
    pub socket_path: String,
}
