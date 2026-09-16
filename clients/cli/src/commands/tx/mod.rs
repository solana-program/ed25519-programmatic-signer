mod approval;
mod sign;
mod sign_only_data;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct TxCommand {
    #[clap(subcommand)]
    command: TxSubcommand,
}

#[derive(Debug, Subcommand)]
enum TxSubcommand {
    /// Review and sign an execution message offline, returning an address/signature pair.
    Sign(sign::SignCommand),
}

pub(crate) fn run(command: TxCommand, client: &Client, output: OutputFormat) -> Result<String> {
    match command.command {
        TxSubcommand::Sign(command) => sign::run(command, client, output),
    }
}
