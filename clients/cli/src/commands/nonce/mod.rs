mod advance;
mod create;
mod show;

use {
    crate::context::CliContext,
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct NonceCommand {
    #[command(subcommand)]
    pub(crate) command: NonceSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum NonceSubcommand {
    /// Create and initialize one SPL Nonce account for a nonce authority.
    Create(create::NonceCreateCommand),
    /// Show the current nonce value, authority, owner, and lamport balance.
    Show(show::NonceShowCommand),
    /// Build a nonce-consuming cancellation transaction for the normal sign and submit path.
    Advance(advance::NonceAdvanceCommand),
}

pub(crate) fn run(command: NonceCommand, context: &CliContext) -> Result<String> {
    match command.command {
        NonceSubcommand::Create(command) => create::run(command, context),
        NonceSubcommand::Show(command) => show::run(command, context),
        NonceSubcommand::Advance(command) => advance::run(command, context),
    }
}
