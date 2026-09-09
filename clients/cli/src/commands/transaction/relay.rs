use {
    crate::{
        artifact,
        client::Client,
        output::{OutputFormat, SimulationOutput, SubmitOutput},
    },
    anyhow::{Context, Result, bail},
    clap::{Args, Subcommand},
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::{VersionedMessage, legacy::Message},
    solana_signature::Signature,
    solana_transaction::versioned::VersionedTransaction,
    spl_programmatic_signer_client::{inspect, nonce::next_nonce, submit_transaction, verify},
    std::{collections::BTreeSet, path::PathBuf},
};

#[derive(Debug, Args)]
pub(crate) struct SubmitCommand {
    transaction: PathBuf,
    #[clap(long = "submit-signer", multiple_occurrences = true)]
    submit_signers: Vec<String>,
    /// Build a relay file without sending. Requires an explicit recent blockhash.
    #[clap(long, requires = "blockhash")]
    no_send: bool,
    #[clap(long, requires = "no-send")]
    blockhash: Option<Hash>,
    #[clap(long, requires = "no-send")]
    outfile: Option<PathBuf>,
}

pub(super) async fn submit(
    command: SubmitCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let transaction = artifact::read(&command.transaction)?;
    if !command.no_send {
        verify_live(&transaction, client).await?;
    }
    let blockhash = match command.blockhash {
        Some(hash) => hash,
        None => client.latest_blockhash().await?,
    };
    let relay = build_relay(&transaction, client, &command.submit_signers, blockhash)?;
    if command.no_send {
        return artifact::write(command.outfile.as_deref(), &relay);
    }
    let signature = client.send_and_confirm_transaction(&relay).await?;
    let summary = inspect(&transaction)?;
    let state = client
        .require_nonce(&summary.nonce_account)
        .await
        .with_context(|| {
            format!(
                "transaction {signature} was confirmed, but its resulting nonce could not be read"
            )
        })?;
    output.render(&SubmitOutput {
        signature,
        nonce_account: summary.nonce_account.to_string(),
        expected_next_nonce: next_nonce(&transaction)?.to_string(),
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
            let summary = inspect(&file)?;
            spl_programmatic_signer_client::verify_genesis_hash(
                &file,
                &client.genesis_hash().await?,
            )?;
            let payer = client.fee_payer()?.try_pubkey()?;
            // Recompile the business instructions with the actual online fee payer. The
            // programmatic owner may hold tokens without any SOL to pay transaction fees.
            let instructions = summary
                .inner_instructions
                .iter()
                .map(|instruction| Instruction {
                    program_id: summary.inner_account_keys
                        [usize::from(instruction.program_id_index)],
                    accounts: instruction
                        .accounts
                        .iter()
                        .map(|index| {
                            let index = usize::from(*index);
                            AccountMeta {
                                pubkey: summary.inner_account_keys[index],
                                is_signer: summary.inner_message.is_signer(index),
                                is_writable: summary
                                    .inner_message
                                    .is_maybe_writable_with_reserved_addresses(
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

async fn verify_live(transaction: &VersionedTransaction, client: &Client) -> Result<()> {
    let summary = inspect(transaction)?;
    let state = client.require_nonce(&summary.nonce_account).await?;
    verify(
        transaction,
        &state,
        &summary.nonce_account,
        &client.genesis_hash().await?,
    )?;
    Ok(())
}

fn build_relay(
    transaction: &VersionedTransaction,
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
    Ok(submit_transaction(
        transaction,
        payer.as_ref(),
        &signer_refs,
        blockhash,
    )?)
}
