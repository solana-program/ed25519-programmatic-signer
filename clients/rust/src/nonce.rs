//! Cancellation and deterministic nonce successors.
use {
    crate::{Result, TransactionPlan, build_transaction, inspect},
    solana_address::Address,
    solana_hash::Hash,
    solana_message::VersionedMessage,
    solana_transaction::versioned::VersionedTransaction,
    spl_legacy_message_executor_interface::instruction::derive_transition_commitment,
    spl_nonce_interface::state::Nonce,
};

/// Builds a cancellation file for a nonce controlled by the authority's ProgrammaticSigner PDA.
/// The file must be signed and submitted before the competing transaction lands.
pub fn advance_transaction(
    nonce_account: Address,
    authority: Address,
    current_nonce: Hash,
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    build_transaction(
        &TransactionPlan::cancellation(nonce_account, authority)?,
        current_nonce,
        genesis_hash,
    )
}

/// Predicts the nonce after this exact inner message succeeds, without contacting RPC.
/// This prediction is conditional: a competing message or cancellation produces a different
/// successor. The transaction need not be signed yet. No account authority is inferred here.
pub fn next_nonce(transaction: &VersionedTransaction) -> Result<Hash> {
    let summary = inspect(transaction)?;
    let VersionedMessage::Legacy(message) = summary.inner_message else {
        return Err(crate::Error::UnsupportedInnerMessage);
    };
    let state = Nonce {
        nonce: message.recent_blockhash,
        authority: Address::default(),
    };
    Ok(state.derive_next_nonce(
        &spl_nonce_interface::id(),
        &summary.nonce_account,
        &derive_transition_commitment(&message),
    ))
}
