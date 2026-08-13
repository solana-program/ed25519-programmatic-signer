use {
    anyhow::{Context, Result, anyhow},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::{VersionedMessage, legacy::Message},
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

pub(crate) fn sign_transaction(
    instructions: &[Instruction],
    fee_payer: &dyn Signer,
    extra_signers: &[&dyn Signer],
    recent_blockhash: Hash,
) -> Result<VersionedTransaction> {
    let fee_payer_address = fee_payer
        .try_pubkey()
        .context("failed to read fee payer pubkey")?;
    let mut signers = Vec::<&dyn Signer>::with_capacity(extra_signers.len().saturating_add(1));
    signers.push(fee_payer);
    for signer in extra_signers {
        signers.push(*signer);
    }
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        instructions,
        Some(&fee_payer_address),
        &recent_blockhash,
    ));
    VersionedTransaction::try_new(message, signers.as_slice())
        .map_err(|error| anyhow!("failed to sign transaction: {error}"))
}

pub(crate) fn unsigned_transaction(message: VersionedMessage) -> VersionedTransaction {
    let required_signatures = usize::from(message.header().num_required_signatures);
    VersionedTransaction {
        signatures: vec![Signature::default(); required_signatures],
        message,
    }
}

pub(crate) fn serialize_transaction_base64(transaction: &VersionedTransaction) -> Result<String> {
    let bytes = wincode::serialize(transaction).context("failed to serialize transaction")?;
    Ok(STANDARD.encode(bytes))
}

pub(crate) fn deserialize_transaction_base64(encoded: &str) -> Result<VersionedTransaction> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .context("failed to decode base64 transaction")?;
    wincode::deserialize_exact::<VersionedTransaction>(&bytes)
        .context("failed to deserialize transaction")
}
