use {
    super::{
        approval::{ApprovalDetails, validate_approval_message},
        sign_only_data::required_authorities,
    },
    crate::{client::Client, commands::tx::sign_only_data, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    clap::{Args, ValueHint},
    indoc::formatdoc,
    serde::Serialize,
    solana_address::Address,
    solana_message::VersionedMessage,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction_status::{Encodable, EncodableWithMeta, UiTransactionEncoding},
    spl_ed25519_signer_client::ProgrammaticSigner,
    std::{collections::BTreeSet, fmt, io, path::PathBuf},
};

#[derive(Debug, Args)]
pub(super) struct SignCommand {
    /// Solana CLI sign-only JSON (`CliSignOnlyData`) with a `base64` message containing
    /// exactly one `Execute` instruction and the default outer blockhash.
    #[clap(value_hint = ValueHint::FilePath)]
    sign_only_file: PathBuf,

    /// Skip the confirmation prompt for non-interactive signers (e.g. file keypairs).
    /// Hardware wallets still require approval on the device.
    #[clap(long)]
    yes: bool,
}

pub(super) fn run(command: SignCommand, client: &Client, output: OutputFormat) -> Result<String> {
    let (outer_message, signed_authorities) = sign_only_data::read_file(&command.sign_only_file)?;
    let approval = validate_approval_message(&outer_message)?;

    let signer = client.default_signer("approval authority")?;
    let signing_authority = signer.try_pubkey()?;
    ensure!(
        required_authorities(&outer_message)?.contains(&signing_authority),
        "{signing_authority} is not an approval authority for this message"
    );

    let summary = render_signing_summary(&approval, &signed_authorities, &signing_authority)?;
    eprintln!("{summary}");

    let signature = sign_outer_message(approval.outer_message, &signer, command.yes)?;
    ensure!(
        signature.verify(signing_authority.as_ref(), &outer_message.serialize()),
        "invalid approval signature"
    );

    output.render(&SignOutput {
        address: signing_authority.to_string(),
        signature: signature.to_string(),
    })
}

#[derive(Serialize)]
struct SignOutput {
    address: String,
    signature: String,
}

impl fmt::Display for SignOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.address, self.signature)
    }
}

fn render_signing_summary(
    approval: &ApprovalDetails<'_>,
    signed_authorities: &BTreeSet<Address>,
    signing_authority: &Address,
) -> Result<String> {
    let inner_ui_message = approval
        .inner_message
        .encode(UiTransactionEncoding::JsonParsed);
    let inner_json = serde_json::to_string_pretty(&inner_ui_message)?;
    let (outer_version, outer_ui_message) = match approval.outer_message {
        VersionedMessage::Legacy(message) => {
            ("legacy", message.encode(UiTransactionEncoding::Json))
        }
        VersionedMessage::V0(message) => ("v0", message.json_encode()),
        VersionedMessage::V1(message) => ("v1", message.encode(UiTransactionEncoding::Json)),
    };
    let outer_json = serde_json::to_string_pretty(&outer_ui_message)?;
    let authorities = required_authorities(approval.outer_message)?;
    let signature_status = authorities
        .iter()
        .map(|authority| {
            let status = if signed_authorities.contains(authority) {
                "present (verified)"
            } else {
                "not included"
            };
            let selected_key_marker = if authority == signing_authority {
                " (your signing key)"
            } else {
                ""
            };
            format!("  {authority}: {status}{selected_key_marker}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let signer_pda =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), signing_authority);
    let present_signatures = signed_authorities.len();
    let required_signatures = authorities.len();
    let executor_program = spl_legacy_message_executor_interface::id();
    let nonce_account = approval.nonce_account;
    let expected_nonce = approval.inner_message.recent_blockhash;
    let message_hash = approval.outer_message.hash();

    Ok(formatdoc! {"
        === Authorization status ===
        Approval message hash: {message_hash}

        Approval signatures: {present_signatures} of {required_signatures} present
        {signature_status}

        PDA your signature authorizes as a signer for Execute:
          {signer_pda}

        === Replay protection ===
        SPL nonce account address: {nonce_account}
        Expected nonce value (inner message's recent blockhash): {expected_nonce}

        === Outer message ===
        One Execute call through the Legacy Message Executor ({executor_program}).
        Your signature authorizes this Execute call, including its accounts, permissions, and \
        the inner message. This message passes the inner message below to the executor.

        {outer_version} message:
        {outer_json}

        === Inner message (what the executor program invokes via CPI) ===
        The Execute instruction's data above is base58-encoded and contains the Execute \
        discriminator followed by the serialized inner message shown below.

        Legacy message:
        {inner_json}

        Signing returns your address and signature. Nothing is submitted."
    })
}

/// Confirm when the signer has no approval step of its own, then sign the outer message.
fn sign_outer_message(
    outer_message: &VersionedMessage,
    signer: &dyn Signer,
    skip_confirmation: bool,
) -> Result<Signature> {
    if !skip_confirmation && !signer.is_interactive() {
        eprint!("Sign this approval? [y/N] ");
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .context("failed to read signing confirmation")?;
        let answer = answer.trim();
        ensure!(
            answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"),
            "signing cancelled"
        );
    }
    signer
        .try_sign_message(&outer_message.serialize())
        .context("failed to sign outer message")
}
