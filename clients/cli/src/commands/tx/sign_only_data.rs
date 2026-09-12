//! Sign-only report file handling and signature bookkeeping for transaction commands.

use {
    anyhow::{Context, Result, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_message::VersionedMessage,
    solana_signature::Signature,
    solana_signer::SignerError,
    std::{collections::BTreeMap, fs::File, io::Write, path::Path, str::FromStr},
};

/// Helpers for reading, writing, and editing a Solana CLI sign-only report.
///
/// The report stores an encoded message and the signatures collected for it. The message
/// specifies the required signer addresses. Adding or replacing a signature here changes only
/// the collected approvals; it never changes that set of required signers.
///
/// Signature updates validate existing and newly supplied signatures, update `signers`,
/// list addresses still missing required signatures in `absent`, and clear `bad_sig`. The original
/// `message` and `blockhash` fields are preserved. If validation fails, the report is unchanged.
///
/// Reading or writing the JSON alone does not validate signatures.
pub trait CliSignOnlyDataExt: Sized {
    /// Read the JSON report without changing or trusting its status metadata.
    fn read(path: &Path) -> Result<Self>;

    /// Write JSON to a new file, never overwriting the destination.
    fn write_new(&self, path: &Path) -> Result<()>;

    /// Deserialize the report's base64-encoded message for inspection or signing.
    fn deserialize_message(&self) -> Result<VersionedMessage>;

    /// Parse and verify every stored signature against this report's message and authorities.
    /// Valid duplicates collapse to one entry; invalid duplicates are still rejected.
    fn verified_signatures(&self) -> Result<BTreeMap<Address, Signature>>;

    /// Add or replace a required authority's signature and refresh the status lists.
    fn add_signature(&mut self, address: Address, signature: Signature) -> Result<()>;
}

impl CliSignOnlyDataExt for CliSignOnlyData {
    fn read(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn write_new(&self, path: &Path) -> Result<()> {
        // Encode before creating the destination so serialization errors leave no file.
        let bytes = serde_json::to_vec_pretty(self)?;
        let mut file = File::create_new(path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }

    fn deserialize_message(&self) -> Result<VersionedMessage> {
        let (_, message) = deserialize_message_with_bytes(self)?;
        Ok(message)
    }

    fn verified_signatures(&self) -> Result<BTreeMap<Address, Signature>> {
        let (bytes, message) = deserialize_message_with_bytes(self)?;
        read_signatures(self, &bytes, required_authorities(&message)?)
    }

    fn add_signature(&mut self, address: Address, signature: Signature) -> Result<()> {
        let (bytes, message) = deserialize_message_with_bytes(self)?;
        let authorities = required_authorities(&message)?;
        let mut signatures = read_signatures(self, &bytes, authorities)?;
        ensure!(
            authorities.contains(&address),
            SignerError::KeypairPubkeyMismatch
        );
        ensure!(
            signature.verify(address.as_ref(), &bytes),
            "invalid approval signature"
        );
        signatures.insert(address, signature);

        // All fallible work finishes before changing the report. Message and blockhash stay intact.
        self.signers.clear();
        self.absent.clear();
        self.bad_sig.clear();
        for address in authorities {
            if let Some(signature) = signatures.get(address) {
                self.signers.push(format!("{address}={signature}"));
            } else {
                self.absent.push(address.to_string());
            }
        }
        Ok(())
    }
}

// Verify signatures against the bytes actually stored in the report, not a reserialization.
fn deserialize_message_with_bytes(data: &CliSignOnlyData) -> Result<(Vec<u8>, VersionedMessage)> {
    let bytes = BASE64_STANDARD
        .decode(
            data.message
                .as_deref()
                .context("missing transaction message")?,
        )
        .context("invalid base64 message")?;
    let message = wincode::deserialize_exact::<VersionedMessage>(&bytes)
        .context("invalid serialized message")?;
    Ok((bytes, message))
}

fn required_authorities(message: &VersionedMessage) -> Result<&[Address]> {
    let authorities = message
        .static_account_keys()
        .get(..usize::from(message.header().num_required_signatures))
        .context("missing approval authority addresses")?;
    ensure!(!authorities.is_empty(), "missing approval authorities");
    Ok(authorities)
}

fn read_signatures(
    data: &CliSignOnlyData,
    bytes: &[u8],
    authorities: &[Address],
) -> Result<BTreeMap<Address, Signature>> {
    let mut signatures = BTreeMap::new();
    for entry in &data.signers {
        let (address, signature) = entry
            .split_once('=')
            .context("invalid signer: expected ADDRESS=SIGNATURE")?;
        let address = Address::from_str(address).context("invalid signer address")?;
        let signature = Signature::from_str(signature).context("invalid signer signature")?;
        ensure!(
            authorities.contains(&address) && signature.verify(address.as_ref(), bytes),
            "invalid existing approval signature"
        );
        signatures.insert(address, signature);
    }
    // `absent` and `badSig` are report summaries, not authority or signature evidence.
    Ok(signatures)
}
