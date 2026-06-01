use {
    crate::nonce::derive_fresh_nonce,
    pinocchio::{AccountView, Address, ProgramResult, error::ProgramError},
    spl_nonce_interface::{error::Error, instruction::AdvanceNonce, state::Nonce},
};

/// Consumes the stored nonce and advances it to a fresh value.
///
/// Callers verify the stored nonce by reading the account before doing their work. The
/// `current_nonce` re-check here makes consumption atomic, so the nonce cannot be spent
/// twice within one transaction, even by nested invocations.
#[inline(never)]
pub fn process_advance(
    program_id: &Address,
    accounts: &mut [AccountView],
    advance: AdvanceNonce,
) -> ProgramResult {
    let [nonce_account, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }
    let data = nonce_account.try_borrow()?;
    // A caller can pre-create a zero-filled program-owned nonce account without
    // initializing it. Those bytes deserialize as a structurally valid zeroed
    // `Nonce`, so reject the uninitialized state before deserializing.
    if data.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidNonceAccount.into());
    }
    let mut state: Nonce =
        wincode::deserialize_exact(&data).map_err(|_| Error::InvalidNonceAccount)?;
    drop(data);

    // The nonce match alone proves nothing about intent because the nonce is public
    // account data. The stored authority's signer privilege proves the account owner
    // authorized consuming it, directly or via a signer program's promoted PDA.
    if authority.address() != &state.authority {
        return Err(Error::AuthorityMismatch.into());
    }
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if advance.current_nonce != state.nonce {
        return Err(Error::NonceMismatch.into());
    }

    state.nonce = derive_fresh_nonce(nonce_account, slot_hashes_account, Some(&state.nonce))?;

    let mut data = nonce_account.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
