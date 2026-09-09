mod advance;
mod create;
mod show;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct NonceCommand {
    #[clap(subcommand)]
    pub(crate) command: NonceSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum NonceSubcommand {
    /// Show the nonce, authority, owner, and balance of an SPL Nonce account.
    Show(show::ShowCommand),
    /// Create and initialize an SPL Nonce account for a nonce authority.
    Create(create::CreateCommand),
    /// Build a cancellation file for a nonce controlled by a cold authority PDA.
    Advance(advance::AdvanceCommand),
}

pub(crate) async fn run(
    command: NonceCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        NonceSubcommand::Advance(command) => advance::run(command, client).await,
        NonceSubcommand::Show(command) => show::run(command, client, output).await,
        NonceSubcommand::Create(command) => create::run(command, client, output).await,
    }
}
