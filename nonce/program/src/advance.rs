use {
    crate::nonce::recent_slot_hash,
    pinocchio::{AccountView, Address, ProgramResult, error::ProgramError},
    spl_nonce_interface::{error::Error, instruction::AdvanceNonceArgs, state::Nonce},
    wincode::ZeroCopy,
};

/// Consumes the stored nonce and advances it to a fresh value.
#[inline(never)]
pub fn process_advance(
    program_id: &Address,
    accounts: &mut [AccountView],
    advance: AdvanceNonceArgs,
) -> ProgramResult {
    let [authority, nonce_account, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }
    if nonce_account.data_len() != Nonce::LEN {
        return Err(Error::InvalidNonceAccount.into());
    }

    // Clone view so address later can be borrowed off the original
    let mut view = nonce_account.clone();

    // A caller-created nonce account remains zero-filled until initialized.
    // Because both `Nonce` fields accept any 32-byte value, wincode accepts
    // the zero-filled data as a valid value. Reject the uninitialized state.
    let mut data = view.try_borrow_mut()?;
    if data.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidNonceAccount.into());
    }
    let state = Nonce::from_bytes_mut(&mut data).map_err(|_| Error::InvalidNonceAccount)?;

    if authority.address() != &state.authority {
        return Err(Error::AuthorityMismatch.into());
    }
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Re-check the expected nonce before advancing it. Once the stored value changes,
    // any attempt to reuse the old nonce will fail.
    if advance.current_nonce != state.nonce {
        return Err(Error::NonceMismatch.into());
    }

    let recent_slot_hash = recent_slot_hash(slot_hashes_account)?;
    state.nonce = state.derive_next_nonce(program_id, nonce_account.address(), &recent_slot_hash);

    Ok(())
}
