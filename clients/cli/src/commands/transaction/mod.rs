mod create;
mod inspect;
mod relay;
mod sign;
mod verify;

use {
    crate::{
        artifact,
        client::Client,
        output::{NextNonceOutput, OutputFormat},
    },
    anyhow::Result,
    clap::{Args, Subcommand},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionCommand {
    #[clap(subcommand)]
    command: TransactionSubcommand,
}

#[derive(Debug, Subcommand)]
enum TransactionSubcommand {
    /// Import Solana or SPL Token sign-only JSON into an unsigned transaction file.
    Create(create::CreateCommand),
    /// Inspect the actual signed message and decoded instructions without RPC.
    Inspect(FileCommand),
    /// Add cold signatures to one or more files without RPC.
    Sign(sign::SignCommand),
    /// Merge valid signatures on copies of exactly the same message without RPC.
    Merge(sign::MergeCommand),
    /// Verify signatures, cluster, and a live or explicitly supplied nonce snapshot.
    Verify(verify::VerifyCommand),
    /// Simulate either the inner instructions or the complete signed relay.
    Simulate(relay::SimulateCommand),
    /// Verify and submit a signed file using the online fee payer.
    Submit(relay::SubmitCommand),
    /// Predict the next nonce after this exact message succeeds, without RPC.
    NextNonce(FileCommand),
}

#[derive(Debug, Args)]
pub(crate) struct FileCommand {
    transaction: PathBuf,
}

pub(crate) async fn run(
    command: TransactionCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        TransactionSubcommand::Create(command) => create::run(command, client).await,
        TransactionSubcommand::Inspect(command) => inspect::run(&command.transaction, output),
        TransactionSubcommand::Sign(command) => sign::run(command, client),
        TransactionSubcommand::Merge(command) => sign::merge(command),
        TransactionSubcommand::Verify(command) => verify::run(command, client, output).await,
        TransactionSubcommand::Simulate(command) => relay::simulate(command, client, output).await,
        TransactionSubcommand::Submit(command) => relay::submit(command, client, output).await,
        TransactionSubcommand::NextNonce(command) => {
            let transaction = artifact::read(&command.transaction)?;
            let summary = spl_programmatic_signer_client::inspect(&transaction)?;
            output.render(&NextNonceOutput {
                nonce_account: summary.nonce_account.to_string(),
                next_nonce: spl_programmatic_signer_client::nonce::next_nonce(&transaction)?
                    .to_string(),
            })
        }
    }
}
