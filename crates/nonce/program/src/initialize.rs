use {
    crate::nonce::derive_fresh_nonce,
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent},
    },
    spl_nonce_interface::state::Nonce,
};

/// Turns a caller-created, program-owned account into a [`Nonce`]
/// bound to `authority` with a fresh nonce.
#[inline(always)]
pub fn process_initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [nonce_account, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // The caller pre-creates the account and assigns this program as its owner
    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // The exact data length pins the layout
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

    let initial_nonce = derive_fresh_nonce(nonce_account, slot_hashes_account, None)?;

    let state = Nonce {
        nonce: initial_nonce,
        authority: Address::from(authority.address()),
    };

    // Write data into the account
    let mut data = nonce_account.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
