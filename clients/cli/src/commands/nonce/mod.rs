pub(crate) mod advance;
pub(crate) mod create;
pub(crate) mod show;

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
    /// Advance an SPL Nonce account, invalidating anything signed against its current value.
    /// Signs directly as the nonce authority, or as the cold authority of a ProgrammaticSigner
    /// PDA nonce authority.
    Advance(advance::AdvanceCommand),
}

pub(crate) async fn run(
    command: NonceCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        NonceSubcommand::Show(command) => show::run(command, client, output).await,
        NonceSubcommand::Create(command) => create::run(command, client, output).await,
        NonceSubcommand::Advance(command) => advance::run(command, client, output).await,
    }
}
