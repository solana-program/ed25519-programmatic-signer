mod decode;
mod sign;
mod submit;
mod summary;

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
    /// Collect signatures for an execute message, then broadcast it in a Submit relay transaction.
    Submit(submit::SubmitCommand),
}

pub(crate) async fn run(
    command: TransactionCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        TransactionSubcommand::Sign(command) => sign::run(command, client, output),
        TransactionSubcommand::Submit(command) => submit::run(command, client, output).await,
    }
}
