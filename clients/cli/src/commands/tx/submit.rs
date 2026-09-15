use {
    super::{approval::validate_approval_message, sign_only_data},
    crate::{client::Client, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    clap::{Args, ValueHint},
    serde::Serialize,
    solana_address::Address,
    solana_message::VersionedMessage,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction::Transaction,
    std::{collections::BTreeMap, fmt, path::PathBuf},
};

#[derive(Debug, Args)]
pub(super) struct SubmitCommand {
    /// Original Solana CLI sign-only JSON (`CliSignOnlyData`) containing the approval message.
    #[clap(value_hint = ValueHint::FilePath)]
    sign_only_file: PathBuf,

    /// Detached approval signature. Repeat for each authority not already signed in the file.
    #[clap(long = "signer", value_name = "ADDRESS=SIGNATURE")]
    signer_entries: Vec<String>,
}

pub(super) async fn run(
    command: SubmitCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let (message, file_signatures) = sign_only_data::read_file(&command.sign_only_file)?;
    let approval = validate_approval_message(&message)?;

    // Combine signatures from the file and --signer arguments
    let all_approval_signatures =
        collect_signatures(&message, file_signatures, &command.signer_entries)?;

    // Check that the live nonce matches the nonce in the approved message.
    let nonce = client.nonce_account(&approval.nonce_account).await?.state;
    ensure!(
        nonce.nonce == approval.inner_message.recent_blockhash,
        "approved message uses nonce {}, but nonce account {} currently has {}",
        approval.inner_message.recent_blockhash,
        approval.nonce_account,
        nonce.nonce
    );

    // Check that the execution message lists the nonce authority as a signer.
    let inner_signer_count = usize::from(approval.inner_message.header.num_required_signatures);
    ensure!(
        approval.inner_message.account_keys[..inner_signer_count].contains(&nonce.authority),
        "execution message does not list nonce authority {} as a signer",
        nonce.authority
    );

    let instruction =
        spl_ed25519_signer_client::instruction::submit(all_approval_signatures, message);
    let fee_payer = client.fee_payer()?;
    let fee_payer_address = fee_payer
        .try_pubkey()
        .context("failed to read fee payer pubkey")?;

    let mut transaction = Transaction::new_with_payer(&[instruction], Some(&fee_payer_address));
    let blockhash = client.latest_blockhash().await?;
    transaction
        .try_sign(&[fee_payer.as_ref()], blockhash)
        .context("failed to sign relay transaction")?;

    let relay_signature = client
        .send_and_confirm_transaction(&transaction)
        .await
        .with_context(|| format!("relay transaction {}", transaction.signatures[0]))?;

    output.render(&SubmitOutput {
        signature: relay_signature,
    })
}

/// Combine verified file signatures with detached approvals, in message authority order.
fn collect_signatures(
    message: &VersionedMessage,
    file_signatures: BTreeMap<Address, Signature>,
    cli_signer_entries: &[String],
) -> Result<Vec<Signature>> {
    let cli_signatures = sign_only_data::verify_supplied_signatures(cli_signer_entries, message)?;
    let mut signatures_by_authority = file_signatures.clone();
    signatures_by_authority.extend(cli_signatures);
    sign_only_data::required_authorities(message)?
        .iter()
        .map(|authority| {
            signatures_by_authority
                .get(authority)
                .copied()
                .with_context(|| format!("missing approval signature for {authority}"))
        })
        .collect()
}

#[derive(Serialize)]
struct SubmitOutput {
    signature: String,
}

impl fmt::Display for SubmitOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.signature)
    }
}
