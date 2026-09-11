use {
    crate::nonce::recent_slot_hash,
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent},
    },
    spl_nonce_interface::state::Nonce,
};

/// Turns a caller-created, program-owned account into a [`Nonce`]
/// bound to `authority` with a fresh nonce value.
#[inline(always)]
pub fn process_initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [nonce_account, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Caller must ensure account is pre-created with authority set to the program
    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    let mut view = nonce_account.clone();
    let mut data = view.try_borrow_mut()?;
    let state = Nonce::view_mut(&mut data).map_err(|_| ProgramError::InvalidAccountData)?;

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

    // Write the initialized state into the account
    *state = Nonce {
        nonce: initial_nonce,
        authority: authority.address().into(),
    };

    Ok(())
}
