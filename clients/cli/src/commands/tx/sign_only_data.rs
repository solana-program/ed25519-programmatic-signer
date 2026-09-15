use {
    anyhow::{Context, Result, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_message::VersionedMessage,
    solana_signature::Signature,
    std::{collections::BTreeMap, path::Path, str::FromStr},
};

/// Read a sign-only file and return its message and the verified approval signatures.
pub(super) fn read_file(path: &Path) -> Result<(VersionedMessage, BTreeMap<Address, Signature>)> {
    let file_bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let data =
        serde_json::from_slice::<CliSignOnlyData>(&file_bytes).context("invalid sign-only JSON")?;
    let encoded = data.message.context("missing transaction message")?;
    let msg_bytes = BASE64_STANDARD
        .decode(encoded)
        .context("invalid base64 message")?;
    let message = wincode::deserialize_exact(&msg_bytes).context("invalid serialized message")?;
    let signed_authorities = verify_supplied_signatures(&data.signers, &message)?;
    Ok((message, signed_authorities))
}

/// The message's required signer addresses, which are the approval authorities.
pub(super) fn required_authorities(message: &VersionedMessage) -> Result<&[Address]> {
    let authorities = message
        .static_account_keys()
        .get(..usize::from(message.header().num_required_signatures))
        .context("missing approval authority addresses")?;
    ensure!(!authorities.is_empty(), "missing approval authorities");
    Ok(authorities)
}

/// Verify each supplied address/signature pair against the message and its required authorities.
/// Valid duplicates collapse to one entry. An invalid duplicate still fails verification.
pub(super) fn verify_supplied_signatures(
    entries: &[String],
    message: &VersionedMessage,
) -> Result<BTreeMap<Address, Signature>> {
    let authorities = required_authorities(message)?;
    let msg_bytes = message.serialize();
    let mut signed_authorities = BTreeMap::new();
    for entry in entries {
        let (address, signature) = entry
            .split_once('=')
            .context("invalid signer: expected ADDRESS=SIGNATURE")?;
        let address = Address::from_str(address).context("invalid signer address")?;
        let signature = Signature::from_str(signature).context("invalid signer signature")?;
        ensure!(
            authorities.contains(&address) && signature.verify(address.as_ref(), &msg_bytes),
            "invalid approval signature for {address}"
        );
        signed_authorities.insert(address, signature);
    }
    Ok(signed_authorities)
}
