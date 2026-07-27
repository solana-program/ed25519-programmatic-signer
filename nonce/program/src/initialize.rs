use {
    crate::nonce::recent_slot_hash,
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent},
    },
    spl_nonce_interface::state::Nonce,
    wincode::ZeroCopy,
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

    // Ensure's the precise length matches the layout
    if nonce_account.data_len() != Nonce::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // A fresh account is zero-filled. Any nonzero byte means it is already initialized.
    let data = nonce_account.try_borrow()?;
    if data.iter().any(|byte| *byte != 0) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    drop(data);

    let rent_required = Rent::get()?.try_minimum_balance(Nonce::LEN)?;
    if nonce_account.lamports() < rent_required {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Read the most recent slot hash to feed the nonce derivation
    let recent_slot_hash = recent_slot_hash(slot_hashes_account)?;
    let initial_nonce =
        Nonce::derive_initial_value(program_id, nonce_account.address(), &recent_slot_hash);

    // Write data into the account
    let mut data = nonce_account.try_borrow_mut()?;
    let state = Nonce::from_bytes_mut(&mut data).map_err(|_| ProgramError::InvalidAccountData)?;
    state.nonce = initial_nonce;
    state.authority.clone_from(authority.address());

    Ok(())
}
