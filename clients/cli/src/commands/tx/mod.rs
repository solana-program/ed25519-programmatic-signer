//! Offline `tx` commands. No Submit relay is constructed or signed here.

mod sign;
pub(crate) mod sign_only_data;

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
    /// Sign an approval transaction with the default signer, without RPC requests or a Submit relay.
    Sign(sign::SignCommand),
}

pub(crate) fn run(command: TxCommand, client: &Client, output: OutputFormat) -> Result<String> {
    match command.command {
        TxSubcommand::Sign(command) => sign::run(command, client, output),
    }
}
