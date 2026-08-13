mod create;
mod inspect;
mod merge;
mod sign;
mod simulate;
mod submit;
mod verify;

use {
    crate::context::CliContext,
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct TransactionCommand {
    #[command(subcommand)]
    pub(crate) command: TransactionSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TransactionSubcommand {
    /// Wrap Solana CLI or SPL Token CLI sign-only JSON into a transaction.
    Create(create::TransactionCreateCommand),
    /// Decode a cold-signed transaction for offline review before signing or submitting.
    Inspect(inspect::TransactionInspectCommand),
    /// Add one or more cold-authority signatures to one or more transactions.
    Sign(sign::TransactionSignCommand),
    /// Combine signatures collected on separate copies of the same transaction.
    Merge(merge::TransactionMergeCommand),
    /// Verify cold-signed transaction structure, signatures, and a nonce account snapshot.
    Verify(verify::TransactionVerifyCommand),
    /// Simulate either the inner message or the hot relay transaction.
    Simulate(simulate::TransactionSimulateCommand),
    /// Build and send the outer Submit transaction for a fully signed transaction.
    Submit(submit::TransactionSubmitCommand),
}

pub(crate) fn run(command: TransactionCommand, context: &CliContext) -> Result<String> {
    match command.command {
        TransactionSubcommand::Create(command) => create::run(command, context),
        TransactionSubcommand::Inspect(command) => inspect::run(command, context.output),
        TransactionSubcommand::Sign(command) => sign::run(command),
        TransactionSubcommand::Merge(command) => merge::run(command),
        TransactionSubcommand::Verify(command) => verify::run(command, context),
        TransactionSubcommand::Simulate(command) => simulate::run(command, context),
        TransactionSubcommand::Submit(command) => submit::run(command, context),
    }
}
