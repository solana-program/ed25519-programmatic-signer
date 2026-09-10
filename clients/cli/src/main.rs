mod artifact;
mod cli;
mod client;
mod commands;
mod transaction;

use {
    anyhow::Result,
    clap::{CommandFactory, FromArgMatches},
    cli::Cli,
    spl_programmatic_signer_cli::output,
};

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let output = commands::run(cli, matches).await?;
    println!("{output}");
    Ok(())
}
