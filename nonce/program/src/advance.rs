use {
    pinocchio::{AccountView, Address, ProgramResult, error::ProgramError},
    solana_hash::Hash,
    spl_nonce_interface::{error::Error, state::Nonce},
};

/// Consumes the stored nonce and advances it to a fresh value.
#[inline(never)]
pub fn process_advance(
    program_id: &Address,
    accounts: &mut [AccountView],
    current_nonce: Hash,
    transition_commitment: Hash,
) -> ProgramResult {
    let [authority, nonce_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // Clone view so address later can be borrowed off the original
    let mut view = nonce_account.clone();

    let mut data = view.try_borrow_mut()?;
    let state = Nonce::view_initialized_mut(&mut data).map_err(|_| Error::InvalidNonceAccount)?;

    if authority.address() != &state.authority {
        return Err(Error::AuthorityMismatch.into());
    }
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Re-check the expected nonce before advancing it. Once the stored value changes,
    // any attempt to reuse the old nonce will fail.
    if current_nonce != state.nonce {
        return Err(Error::NonceMismatch.into());
    }

    state.nonce =
        state.derive_next_nonce(program_id, nonce_account.address(), &transition_commitment);

    Ok(())
}
