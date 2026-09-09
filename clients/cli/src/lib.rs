mod cli;
mod client;
mod commands;
pub mod output;

use {
    anyhow::Result,
    clap::{CommandFactory, FromArgMatches},
    cli::Cli,
};

/// Parses command-line arguments and runs the requested command.
pub async fn run() -> Result<()> {
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let output = commands::run(cli, matches).await?;
    println!("{output}");
    Ok(())
}
