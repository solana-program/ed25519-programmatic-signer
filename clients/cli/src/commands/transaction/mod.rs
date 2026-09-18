mod sign;
mod sign_only_data;

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
    /// Review and sign an execution message offline, returning an address/signature pair.
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
