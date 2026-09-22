use {
    crate::{cli::keypair_source_parser, client::Client, output::OutputFormat},
    anyhow::{Context, Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    indoc::formatdoc,
    serde::Serialize,
    solana_address::Address,
    solana_clap_v3_utils::input_parsers::signer::SignerSource,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy::Message},
    solana_sanitize::Sanitize,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction_status::{Encodable, EncodableWithMeta, UiTransactionEncoding},
    spl_ed25519_signer_client::{ProgrammaticSigner, message::wrapped_message},
    spl_legacy_message_executor_client::instruction::execute,
    std::{collections::BTreeSet, fmt, io},
};

#[derive(Debug, Args)]
pub(super) struct SignCommand {
    /// Base64-encoded legacy inner transaction message.
    #[clap(long)]
    inner_message: String,

    /// SPL nonce account protecting this execution.
    #[clap(long)]
    nonce_account: Address,

    /// Nonce account authority.
    #[clap(long)]
    nonce_authority: Address,

    /// Expected nonce value, which replaces the inner message's recent blockhash.
    #[clap(long)]
    nonce_hash: Hash,

    /// Full set of authorities promoting their derived PDA signers. Repeat for each authority.
    /// Each derived signer must be the nonce authority or a signer on the inner message.
    /// Addresses are sorted and de-duplicated before constructing the wrapped message.
    #[clap(long, required = true)]
    authority: Vec<Address>,

    /// Signer source: a keypair file, usb:// URL, prompt:// URL, or the ASK keyword.
    /// Repeat to sign with multiple local keys. Each must be in --authority.
    /// Defaults to the configured keypair.
    #[clap(long, value_parser = keypair_source_parser())]
    signer: Vec<SignerSource>,

    /// Hide the signing summary. Confirmation prompts and errors are still shown.
    #[clap(long)]
    quiet: bool,

    /// Skip the confirmation prompt for non-interactive signers (e.g. file keypairs).
    /// Hardware wallets still require approval on the device.
    #[clap(long)]
    yes: bool,
}

pub(super) fn run(command: SignCommand, client: &Client, output: OutputFormat) -> Result<String> {
    let mut inner = read_message(&command.inner_message)?;
    inner.recent_blockhash = command.nonce_hash;
    // Sort/dedupe so participants construct identical messages.
    let mut authorities = command.authority;
    authorities.sort_unstable();
    authorities.dedup();
    let instruction = execute(&command.nonce_account, &command.nonce_authority, &inner);
    // wrapped_message compiles u8 indices and casts header counts. Reject oversized inputs
    // before invoking it, so malformed input cannot panic or truncate the counts.
    let account_count = instruction
        .accounts
        .iter()
        .map(|meta| meta.pubkey)
        .chain(authorities.iter().copied())
        .chain(std::iter::once(instruction.program_id))
        .collect::<BTreeSet<_>>()
        .len();
    ensure!(
        account_count <= 256,
        "too many accounts for the wrapped message"
    );
    let inner_signers = &inner.account_keys[..usize::from(inner.header.num_required_signatures)];
    let derived_signers = authorities
        .iter()
        .map(|authority| {
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority)
        })
        .collect::<BTreeSet<_>>();
    // Ordinary signers need a slot in the wrapped message header to forward their
    // submission signature. Their wrapped-message approvals must also be collected.
    let wrapped_signers = authorities
        .iter()
        .copied()
        .chain(
            inner_signers
                .iter()
                .chain(std::iter::once(&command.nonce_authority))
                .filter(|address| !derived_signers.contains(*address))
                .copied(),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ensure!(
        wrapped_signers.len() < 128,
        "too many required signers for a legacy message"
    );
    for authority in &authorities {
        let pda = ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority);
        ensure!(
            pda == command.nonce_authority || inner_signers.contains(&pda),
            "Authority {authority}'s derived signer {pda} is neither the nonce authority nor a \
             signer on the inner message"
        );
    }
    let outer = wrapped_message(&instruction, &wrapped_signers);
    outer.sanitize().context("invalid wrapped message")?;
    let execute_message = BASE64_STANDARD.encode(outer.serialize());

    let signers = if command.signer.is_empty() {
        vec![client.load_signer_or_config_default(None, "message authority")?]
    } else {
        command
            .signer
            .iter()
            .map(|source| client.load_signer(source, "message authority"))
            .collect::<Result<Vec<_>>>()?
    };
    let mut unique_signers = Vec::new();
    for signer in signers {
        let address = signer.try_pubkey()?;
        ensure!(
            authorities.contains(&address),
            "Signer {address} is not in the supplied --authority list"
        );
        if !unique_signers
            .iter()
            .any(|(existing, _)| existing == &address)
        {
            unique_signers.push((address, signer));
        }
    }

    if !command.quiet {
        eprintln!(
            "{}",
            render_signing_summary(
                &inner,
                &outer,
                &command.nonce_account,
                &command.nonce_authority,
                &authorities
            )?
        );
    }
    let mut entries = Vec::new();
    for (authority, signature) in sign_outer_message(&outer, &unique_signers, command.yes)? {
        entries.push(SignOutput {
            address: authority.to_string(),
            signature: signature.to_string(),
            execute_message: execute_message.clone(),
        });
    }
    output.render(&SignOutputs(entries))
}

fn read_message(input: &str) -> Result<Message> {
    let bytes = BASE64_STANDARD
        .decode(input.trim())
        .context("invalid base64 message")?;
    let message: VersionedMessage =
        wincode::deserialize_exact(&bytes).context("invalid serialized message")?;
    let VersionedMessage::Legacy(message) = message else {
        bail!("transaction sign supports only legacy inner messages");
    };
    message.sanitize().context("invalid inner message")?;
    ensure!(
        !message.has_duplicates(),
        "inner message must not contain duplicate account keys"
    );
    Ok(message)
}

#[derive(Serialize)]
struct SignOutput {
    address: String,
    signature: String,
    execute_message: String,
}

#[derive(Serialize)]
#[serde(transparent)]
struct SignOutputs(Vec<SignOutput>);

impl fmt::Display for SignOutputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for entry in &self.0 {
            writeln!(f, "Address: {}", entry.address)?;
            writeln!(f, "Signature: {}", entry.signature)?;
            writeln!(f)?;
        }
        if let Some(entry) = self.0.first() {
            write!(f, "Execute message (base64):\n{}", entry.execute_message)?;
        }
        Ok(())
    }
}

fn render_signing_summary(
    inner: &Message,
    outer: &VersionedMessage,
    nonce_account: &Address,
    nonce_authority: &Address,
    authorities: &[Address],
) -> Result<String> {
    let inner_json =
        serde_json::to_string_pretty(&inner.encode(UiTransactionEncoding::JsonParsed))?;
    let (outer_version, outer_ui_message) = match outer {
        VersionedMessage::Legacy(message) => {
            ("Legacy", message.encode(UiTransactionEncoding::Json))
        }
        VersionedMessage::V0(message) => ("v0", message.json_encode()),
        VersionedMessage::V1(message) => ("v1", message.encode(UiTransactionEncoding::Json)),
    };
    let outer_json = serde_json::to_string_pretty(&outer_ui_message)?;
    let signing_keys = authorities
        .iter()
        .map(|authority| {
            let pda =
                ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority);
            format!("  {authority} (derived signer: {pda})")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let forwarded_signers = outer.static_account_keys()
        [..usize::from(outer.header().num_required_signatures)]
        .iter()
        .filter(|address| !authorities.contains(address))
        .map(|address| format!("  {address}"))
        .collect::<Vec<_>>();
    let forwarded_section = if forwarded_signers.is_empty() {
        String::new()
    } else {
        format!(
            "\nForwarded signers (sign at submission):\n{}\n",
            forwarded_signers.join("\n")
        )
    };
    let message_hash = outer.hash();
    let expected_nonce = inner.recent_blockhash;
    Ok(formatdoc! {"
        === Authorization ===
        Message hash: {message_hash}
        PDA promotion authorities:
        {signing_keys}
        {forwarded_section}
        === Replay protection ===
        SPL nonce account address: {nonce_account}
        Nonce authority: {nonce_authority}
        Expected nonce value (inner message's recent blockhash): {expected_nonce}

        === Outer message ===
        Your signatures authorize this Execute call, including its accounts, permissions,
        and the inner message.

        {outer_version} message:
        {outer_json}

        === Inner message (what the executor program invokes via CPI) ===
        Legacy message:
        {inner_json}

        Signing returns addresses, signatures, and the base64 Execute message. Nothing is submitted."
    })
}

/// Confirm once before signing if any signer has no approval step of its own.
fn sign_outer_message(
    outer_message: &VersionedMessage,
    signers: &[(Address, Box<dyn Signer>)],
    skip_confirmation: bool,
) -> Result<Vec<(Address, Signature)>> {
    if !skip_confirmation && signers.iter().any(|(_, signer)| !signer.is_interactive()) {
        let addresses = signers
            .iter()
            .map(|(address, _)| address.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprint!("Sign this message for {addresses}? [y/N] ");
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
    let message_bytes = outer_message.serialize();
    signers
        .iter()
        .map(|(authority, signer)| {
            let signature = signer
                .try_sign_message(&message_bytes)
                .with_context(|| format!("failed to sign outer message with {authority}"))?;
            Ok((*authority, signature))
        })
        .collect()
}
