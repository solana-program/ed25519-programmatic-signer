mod stake;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(super) struct PrepareCommand {
    #[clap(subcommand)]
    command: PrepareSubcommand,
}

#[derive(Debug, Subcommand)]
enum PrepareSubcommand {
    /// Build an unsigned transaction to transfer stake authority to a programmatic signer.
    Stake(stake::StakeCommand),
}

pub(super) async fn run(
    command: PrepareCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        PrepareSubcommand::Stake(command) => stake::run(command, client, output).await,
    }
}
