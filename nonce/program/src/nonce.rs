use {
    pinocchio::{AccountView, error::ProgramError, sysvars::slot_hashes::SlotHashes},
    solana_hash::Hash,
};

/// Reads the most recent hash from the `SlotHashes` sysvar.
#[inline(always)]
pub fn recent_slot_hash(slot_hashes_account: &AccountView) -> Result<Hash, ProgramError> {
    let slot_hashes = SlotHashes::from_account_view(slot_hashes_account)?;
    let recent_slot_hash = &slot_hashes
        .get_entry(0)
        .ok_or(ProgramError::InvalidArgument)?
        .hash;

    Ok(Hash::new_from_array(*recent_slot_hash))
}
