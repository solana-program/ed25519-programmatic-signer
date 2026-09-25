mod sign;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct TransactionCommand {
    #[clap(subcommand)]
    command: TransactionSubcommand,
}

#[derive(Debug, Subcommand)]
enum TransactionSubcommand {
    /// Wrap and sign an inner message offline, returning signatures and the Execute message.
    Sign(sign::SignCommand),
}

pub(crate) fn run(
    command: TransactionCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        TransactionSubcommand::Sign(command) => sign::run(command, client, output),
    }
}
