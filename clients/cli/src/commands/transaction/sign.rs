use {
    super::{
        decode::read_execution_message,
        summary::{
            SummarySigners, confirm_signing, next_nonce, render_signing_summary,
            sign_authorization_message,
        },
        v1_transaction::{ensure_transaction_size, relay_transaction},
    },
    crate::{cli::keypair_source_parser, client::Client, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    serde::Serialize,
    solana_address::Address,
    solana_clap_v3_utils::input_parsers::signer::SignerSource,
    solana_hash::Hash,
    solana_message::{VersionedMessage, v1},
    solana_signature::Signature,
    solana_signer::Signer,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_message_executor_client::instruction::execute,
    std::{collections::BTreeSet, fmt},
};

#[derive(Debug, Args)]
pub(super) struct SignCommand {
    /// Base64-encoded v1 execution message.
    #[clap(long)]
    execution_message: String,

    /// SPL nonce account protecting this execution.
    #[clap(long)]
    nonce_account: Address,

    /// Nonce account authority.
    #[clap(long)]
    nonce_authority: Address,

    /// Expected nonce value, which replaces the execution message's recent blockhash. Pass the
    /// current value of the given nonce account to allow this transaction to be executed
    /// immediately. Pass the next nonce value from an earlier `transaction sign` to sign a message
    /// that can only execute after that one.
    #[clap(long)]
    nonce_hash: Hash,

    /// Full set of authorities promoting their derived PDA signers. Repeat for each authority.
    /// Each derived signer must be the nonce authority or a signer on the execution message.
    /// Addresses are sorted and de-duplicated before constructing the authorization message.
    #[clap(long, required = true)]
    authority: Vec<Address>,

    /// Address that must approve the submission, such as a co-signing service, without the
    /// executor using it. Repeat for each signer. It gets a signer slot on the authorization
    /// message, filled at submission by its relay transaction signature or its own signature over
    /// the authorization message. It must not be an --authority or a signer the executor uses.
    #[clap(long, value_name = "ADDRESS")]
    required_relay_signer: Vec<Address>,

    /// Remove transaction config fields (such as compute unit limit and priority fee) from the
    /// execution message instead of rejecting it. These fields only apply to top-level
    /// transactions and are ignored by the executor.
    #[clap(long)]
    strip_transaction_config: bool,

    /// Signer source: a keypair file, usb:// URL, prompt:// URL, or the ASK keyword.
    /// Repeat to sign with multiple local keys. Each must be in --authority.
    /// Omit to print the authorization message without signing it, for authorities that sign it
    /// externally, such as a custody system or HSM.
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
    let mut execution_message =
        read_execution_message(&command.execution_message, command.strip_transaction_config)?;
    execution_message.lifetime_specifier = command.nonce_hash;
    // Sort/dedupe so participants construct identical messages.
    let mut authorities = command.authority;
    authorities.sort_unstable();
    authorities.dedup();
    let mut required_relay_signers = command.required_relay_signer;
    required_relay_signers.sort_unstable();
    required_relay_signers.dedup();
    let instruction = execute(
        &command.nonce_account,
        &command.nonce_authority,
        &execution_message,
    );
    let execution_signers = &execution_message.account_keys
        [..usize::from(execution_message.header.num_required_signatures)];
    let derived_signers = authorities
        .iter()
        .map(|authority| {
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority)
        })
        .collect::<BTreeSet<_>>();
    // Signers the executor uses directly must sign at submission, including authorities that
    // are also used directly. Derived PDAs are promoted by Submit instead.
    let forwarded_signers = execution_signers
        .iter()
        .chain(std::iter::once(&command.nonce_authority))
        .filter(|address| !derived_signers.contains(*address))
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    // Forwarded signers need a slot in the authorization message header to forward their
    // submission signature. Their authorization message approvals must also be collected.
    let authorization_signers = authorities
        .iter()
        .chain(&forwarded_signers)
        .chain(&required_relay_signers)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    // Every authority is an authorization signer, so this also bounds the authorization message's
    // account keys well below the 256 at which authorization_message panics compiling u8 indexes.
    // Check it before building the message.
    ensure!(
        authorization_signers.len() <= usize::from(v1::MAX_SIGNATURES),
        "too many required signers for the authorization message: {} exceeds the v1 limit of {}",
        authorization_signers.len(),
        v1::MAX_SIGNATURES
    );
    for authority in &authorities {
        let pda = ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority);
        ensure!(
            pda == command.nonce_authority || execution_signers.contains(&pda),
            "Authority {authority}'s derived signer {pda} is neither the nonce authority nor a \
             signer on the execution message"
        );
    }
    // Only allow adding required relay signers who don't otherwise need to sign
    for signer in &required_relay_signers {
        ensure!(
            !authorities.contains(signer),
            "{signer} is both an --authority and a --required-relay-signer"
        );
        ensure!(
            *signer != command.nonce_authority && !execution_signers.contains(signer),
            "Required relay signer {signer} is the nonce authority or a signer on the execution \
             message, so it already signs"
        );
        let pda = ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), signer);
        ensure!(
            pda != command.nonce_authority && !execution_signers.contains(&pda),
            "Required relay signer {signer}'s derived signer {pda} is the nonce authority or a \
             signer on the execution message, so its signature would promote it; pass it with \
             --authority instead"
        );
    }
    let VersionedMessage::V1(authorization_message) =
        spl_ed25519_signer_client::message::authorization_message(
            &instruction,
            &authorization_signers,
        )
    else {
        unreachable!("authorization_message builds a v1 message");
    };
    // Validate the v1 message directly. Its errors name the violated limit, which sanitize's
    // generic errors do not.
    authorization_message
        .validate()
        .context("invalid authorization message")?;
    ensure_relay_transaction_fits(&authorization_message, &authorities, &forwarded_signers)?;
    let encoded_authorization_message = BASE64_STANDARD.encode(authorization_message.serialize());
    let authorization_message_hash =
        VersionedMessage::hash_raw_message(&authorization_message.serialize()).to_string();
    let execution_message_hash =
        VersionedMessage::hash_raw_message(&execution_message.serialize()).to_string();
    let next_nonce = next_nonce(&command.nonce_account, &execution_message).to_string();

    let signers = command
        .signer
        .iter()
        .map(|source| client.load_signer(source, "message authority", false))
        .collect::<Result<Vec<_>>>()?;
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
                &execution_message,
                &authorization_message,
                &command.nonce_account,
                &command.nonce_authority,
                &SummarySigners {
                    authorities: &authorities,
                    forwarded: &forwarded_signers,
                    required_relay: &required_relay_signers,
                },
                if unique_signers.is_empty() {
                    "Returns forwarded signers, the consumed and next nonce values, message \
                     hashes, a timestamp, and the base64 authorization message for external \
                     signers. Nothing is signed or submitted."
                } else {
                    "Signing returns addresses, signatures, forwarded signers, the consumed and \
                     next nonce values, message hashes, a timestamp, and the base64 authorization \
                     message. Nothing is submitted."
                },
            )?
        );
    }
    confirm_signing(&unique_signers, command.yes)?;
    let signatures = sign_authorization_message(&authorization_message, &unique_signers)?
        .into_iter()
        .map(|(authority, signature)| AuthoritySignature {
            address: authority.to_string(),
            signature: signature.to_string(),
        })
        .collect();
    output.render(&SignOutput {
        authorization_message: encoded_authorization_message,
        authorization_message_hash,
        execution_message_hash,
        consumed_nonce: command.nonce_hash.to_string(),
        next_nonce,
        forwarded_signers: forwarded_signers.iter().map(ToString::to_string).collect(),
        required_relay_signers: required_relay_signers
            .iter()
            .map(ToString::to_string)
            .collect(),
        signatures,
        timestamp: format!("{:.0}", jiff::Timestamp::now()),
    })
}

#[derive(Serialize)]
struct SignOutput {
    authorization_message: String,
    /// Hash of the authorization message, which the signatures cover.
    authorization_message_hash: String,
    /// Hash of the execution message, after its recent blockhash is replaced by the consumed nonce.
    execution_message_hash: String,
    /// The nonce value the execution consumes, which is the execution message's recent blockhash.
    consumed_nonce: String,
    next_nonce: String,
    forwarded_signers: Vec<String>,
    required_relay_signers: Vec<String>,
    signatures: Vec<AuthoritySignature>,
    /// UTC time the output was produced, in RFC 3339 format.
    timestamp: String,
}

#[derive(Serialize)]
struct AuthoritySignature {
    address: String,
    signature: String,
}

impl fmt::Display for SignOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for entry in &self.signatures {
            writeln!(f, "Address: {}", entry.address)?;
            writeln!(f, "Signature: {}", entry.signature)?;
            writeln!(f)?;
        }
        if !self.forwarded_signers.is_empty() {
            writeln!(f, "Forwarded signers (sign at submission):")?;
            for address in &self.forwarded_signers {
                writeln!(f, "  {address}")?;
            }
            writeln!(f)?;
        }
        if !self.required_relay_signers.is_empty() {
            writeln!(f, "Required relay signers (sign at submission):")?;
            for address in &self.required_relay_signers {
                writeln!(f, "  {address}")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "Consumed nonce value: {}", self.consumed_nonce)?;
        writeln!(f, "Next nonce value (after execution): {}", self.next_nonce)?;
        writeln!(f, "Execution message hash: {}", self.execution_message_hash)?;
        writeln!(
            f,
            "Authorization message hash: {}",
            self.authorization_message_hash
        )?;
        writeln!(f, "Timestamp: {}", self.timestamp)?;
        writeln!(f)?;
        write!(
            f,
            "Authorization message (base64):\n{}",
            self.authorization_message
        )
    }
}

/// Check that the smallest relay transaction `transaction submit` could build for the
/// authorization message fits within the transaction size limit, so it can be submitted. The
/// actual one is checked at submission, and may be larger, for example if there are extra accounts or a durable nonce.
fn ensure_relay_transaction_fits(
    authorization_message: &v1::Message,
    authorities: &[Address],
    forwarded_signers: &[Address],
) -> Result<()> {
    let authorization_signers = &authorization_message.account_keys
        [..usize::from(authorization_message.header.num_required_signatures)];
    // Slots in Submit's data for authorization message signatures. Authorities must sign the
    // authorization message. Forwarded signers that are not authorities sign the relay transaction
    // instead, leaving their slot here empty.
    let signatures = authorization_signers
        .iter()
        .map(|address| {
            (authorities.contains(address) || !forwarded_signers.contains(address))
                .then(Signature::default)
        })
        .collect();
    // Make any authorization signer the fee payer
    // Infallible: --authority is required, so there is at least one authorization signer.
    let fee_payer = forwarded_signers
        .first()
        .unwrap_or(&authorization_signers[0]);
    let transaction = relay_transaction(
        authorization_message,
        signatures,
        forwarded_signers,
        fee_payer,
        None,
        Hash::default(),
    )?;
    ensure_transaction_size(
        "the smallest relay transaction for this authorization message",
        &transaction,
    )
}
