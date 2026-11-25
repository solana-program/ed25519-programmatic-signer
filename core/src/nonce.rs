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
