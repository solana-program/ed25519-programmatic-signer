mod approval;
mod sign;
mod sign_only_data;
mod submit;

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
    /// Wrap an approved execution message in a relay transaction and broadcast it.
    Submit(submit::SubmitCommand),
}

pub(crate) async fn run(
    command: TxCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        TxSubcommand::Sign(command) => sign::run(command, client, output),
        TxSubcommand::Submit(command) => submit::run(command, client, output).await,
    }
}
