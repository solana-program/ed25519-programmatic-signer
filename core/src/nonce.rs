use {
    crate::state::Nonce,
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, clock::Clock, rent::Rent, slot_hashes::SlotHashes},
    },
    solana_hash::Hash,
};

/// Reads the most recent hash from the `SlotHashes` sysvar.
#[inline(always)]
fn recent_slot_hash(slot_hashes_account: &AccountView) -> Result<Hash, ProgramError> {
    let slot_hashes = SlotHashes::from_account_view(slot_hashes_account)?;
    let recent_slot_hash = &slot_hashes
        .get_entry(0)
        .ok_or(ProgramError::InvalidArgument)?
        .hash;

    Ok(Hash::new_from_array(*recent_slot_hash))
}

/// Turns a caller-created, program-owned account into a [`Nonce`]
/// bound to `authority` with a fresh nonce value.
#[inline(always)]
pub fn process_initialize_nonce(
    program_id: &Address,
    accounts: &mut [AccountView],
) -> ProgramResult {
    let [nonce_account, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Caller must ensure account is pre-created with authority set to the program
    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    let mut view = nonce_account.clone();
    let mut data = view.try_borrow_mut()?;
    let state = Nonce::view_maybe_uninitialized_mut(&mut data)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    // Initialization requires zeroed state
    if state.is_initialized() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    let rent_required = Rent::get()?.try_minimum_balance(Nonce::LEN)?;
    if nonce_account.lamports() < rent_required {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Read the most recent slot hash to feed the nonce derivation
    let recent_slot_hash = recent_slot_hash(slot_hashes_account)?;
    let initial_nonce =
        Nonce::derive_initial_nonce(program_id, nonce_account.address(), &recent_slot_hash);
    let initialize_slot = Clock::get()?.slot;

    // Write the initialized state into the account
    *state = Nonce {
        nonce: initial_nonce,
        authority: authority.address().into(),
        initialize_slot: initialize_slot.into(),
    };

    Ok(())
}

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
    let [authority, transition_signer, nonce_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // Clone view so address later can be borrowed off the original
    let mut view = nonce_account.clone();

    let mut data = view.try_borrow_mut()?;
    let state = Nonce::view_mut(&mut data).map_err(|_| Error::InvalidNonceAccount)?;

    if authority.address() != &state.authority {
        return Err(Error::AuthorityMismatch.into());
    }
    if !authority.is_signer() || !transition_signer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Re-check the expected nonce before advancing it. Once the stored value changes,
    // any attempt to reuse the old nonce will fail.
    if current_nonce != state.nonce {
        return Err(Error::NonceMismatch.into());
    }

    state.nonce = state.derive_next_nonce(
        program_id,
        nonce_account.address(),
        transition_signer.address(),
        &transition_commitment,
    );

    Ok(())
}
