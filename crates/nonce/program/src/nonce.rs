use {
    pinocchio::{AccountView, error::ProgramError, sysvars::slot_hashes::SlotHashes},
    solana_hash::Hash,
    spl_nonce_interface::state::NONCE_DERIVATION_TAG,
};

/// Derives the nonce value written into a nonce account.
#[inline(always)]
pub fn derive_fresh_nonce(
    nonce_account: &AccountView,
    slot_hashes_account: &AccountView,
    previous_nonce: Option<&Hash>,
) -> Result<Hash, ProgramError> {
    let slot_hashes = SlotHashes::from_account_view(slot_hashes_account)?;
    let recent_slot_hash = slot_hashes
        .get_entry(0)
        .ok_or(ProgramError::InvalidArgument)?
        .hash;

    let previous_nonce = previous_nonce.map(AsRef::as_ref).unwrap_or_default();

    Ok(solana_sha256_hasher::hashv(&[
        NONCE_DERIVATION_TAG,
        &nonce_account.address().to_bytes(),
        previous_nonce,
        &recent_slot_hash,
    ]))
}
