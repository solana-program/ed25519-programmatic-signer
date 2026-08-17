mod cli;
mod client;
mod commands;
mod output;

use {anyhow::Result, clap::Parser, cli::Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = commands::run(cli).await?;
    println!("{output}");
    Ok(())
}
