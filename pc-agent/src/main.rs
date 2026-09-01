//! Binary entry point — thin wrapper around the library `run` function.

use clap::Parser;
use pc_agent::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    pc_agent::run(cli).await
}
