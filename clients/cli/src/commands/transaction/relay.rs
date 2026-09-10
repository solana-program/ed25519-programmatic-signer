use {
    crate::{
        artifact,
        client::Client,
        output::{OutputFormat, SimulationOutput, SubmitOutput},
        transaction::WrappedTransaction,
    },
    anyhow::{Context, Result, bail},
    clap::{Args, Subcommand},
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::{VersionedMessage, legacy::Message},
    solana_signature::Signature,
    solana_transaction::versioned::VersionedTransaction,
    std::{collections::BTreeSet, path::PathBuf},
};

#[derive(Debug, Args)]
pub(crate) struct SubmitCommand {
    transaction: PathBuf,
    #[clap(long = "submit-signer", multiple_occurrences = true)]
    submit_signers: Vec<String>,
}

pub(super) async fn submit(
    command: SubmitCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let transaction = artifact::read(&command.transaction)?;
    verify_live(&transaction, client).await?;
    let relay = build_relay(
        &transaction,
        client,
        &command.submit_signers,
        client.latest_blockhash().await?,
    )?;
    let signature = client.send_and_confirm_transaction(&relay).await?;
    let state = client
        .require_nonce(transaction.nonce_account())
        .await
        .with_context(|| {
            format!(
                "transaction {signature} was confirmed, but its resulting nonce could not be read"
            )
        })?;
    output.render(&SubmitOutput {
        signature,
        nonce_account: transaction.nonce_account().to_string(),
        expected_next_nonce: transaction.next_nonce().to_string(),
        observed_nonce: state.nonce.to_string(),
    })
}

#[derive(Debug, Args)]
pub(crate) struct SimulateCommand {
    #[clap(subcommand)]
    mode: SimulateMode,
}

#[derive(Debug, Subcommand)]
enum SimulateMode {
    /// Simulate business instructions without checking the nonce or programmatic signatures.
    Inner(super::FileCommand),
    /// Verify and simulate the entire relay, including all three programs.
    Relay(RelayCommand),
}

#[derive(Debug, Args)]
struct RelayCommand {
    transaction: PathBuf,
    #[clap(long = "submit-signer", multiple_occurrences = true)]
    submit_signers: Vec<String>,
}

pub(super) async fn simulate(
    command: SimulateCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let (mode, transaction, verify_signatures) = match command.mode {
        SimulateMode::Inner(command) => {
            let file = artifact::read(&command.transaction)?;
            file.verify_genesis_hash(&client.genesis_hash().await?)?;
            let inner = file.inner();
            let payer = client.fee_payer()?.try_pubkey()?;
            // Recompile the business instructions with the actual online fee payer. The
            // programmatic owner may hold tokens without any SOL to pay transaction fees.
            let instructions = inner
                .instructions
                .iter()
                .map(|instruction| Instruction {
                    program_id: inner.account_keys[usize::from(instruction.program_id_index)],
                    accounts: instruction
                        .accounts
                        .iter()
                        .map(|index| {
                            let index = usize::from(*index);
                            AccountMeta {
                                pubkey: inner.account_keys[index],
                                is_signer: inner.is_signer(index),
                                is_writable: inner.is_maybe_writable_with_reserved_addresses(
                                    index,
                                    None::<&BTreeSet<_>>,
                                ),
                            }
                        })
                        .collect(),
                    data: instruction.data.clone(),
                })
                .collect::<Vec<_>>();
            let message = VersionedMessage::Legacy(Message::new(&instructions, Some(&payer)));
            let transaction = VersionedTransaction {
                signatures: vec![
                    Signature::default();
                    usize::from(message.header().num_required_signatures)
                ],
                message,
            };
            ("inner", transaction, false)
        }
        SimulateMode::Relay(command) => {
            let file = artifact::read(&command.transaction)?;
            verify_live(&file, client).await?;
            let transaction = build_relay(
                &file,
                client,
                &command.submit_signers,
                client.latest_blockhash().await?,
            )?;
            ("relay", transaction, true)
        }
    };
    let result = client.simulate(&transaction, verify_signatures).await?;
    if let Some(error) = &result.err {
        bail!(
            "{mode} simulation failed: {error:?}\n{}",
            result.logs.as_deref().unwrap_or_default().join("\n")
        );
    }
    output.render(&SimulationOutput {
        mode: mode.into(),
        units_consumed: result.units_consumed,
        logs: result.logs.unwrap_or_default(),
    })
}

async fn verify_live(transaction: &WrappedTransaction, client: &Client) -> Result<()> {
    let state = client.require_nonce(transaction.nonce_account()).await?;
    transaction.verify(&state, &client.genesis_hash().await?)
}

fn build_relay(
    transaction: &WrappedTransaction,
    client: &Client,
    sources: &[String],
    blockhash: Hash,
) -> Result<VersionedTransaction> {
    let payer = client.fee_payer()?;
    let signers = sources
        .iter()
        .map(|source| client.signer(source, "submit signer"))
        .collect::<Result<Vec<_>>>()?;
    let signer_refs = signers
        .iter()
        .map(|signer| signer.as_ref())
        .collect::<Vec<_>>();
    transaction.relay(payer.as_ref(), &signer_refs, blockhash)
}
