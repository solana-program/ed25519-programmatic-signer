use {
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent, slot_hashes::SlotHashes},
    },
    spl_ed25519_programmatic_signer_interface::state::{
        INIT_NONCE_DERIVATION_TAG, ProgrammaticSignerAccount,
    },
};

/// Turns a caller-created, program-owned account into a [`ProgrammaticSignerAccount`]
/// bound to `authority` with a fresh nonce.
#[inline(always)]
pub fn process_initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [
        programmatic_signer_account,
        authority,
        slot_hashes_account,
        ..,
    ] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Caller must ensure account is pre-created with authority set to the program
    if !programmatic_signer_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // Ensure's the precise length matches the layout
    if programmatic_signer_account.data_len() != ProgrammaticSignerAccount::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // A fresh account is zero-filled. Any nonzero byte means it is already initialized.
    let data = programmatic_signer_account.try_borrow()?;
    if data.iter().any(|byte| *byte != 0) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    drop(data);

    let rent_required = Rent::get()?.try_minimum_balance(ProgrammaticSignerAccount::LEN)?;
    if programmatic_signer_account.lamports() < rent_required {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Read the most recent slot hash to feed the nonce derivation
    let slot_hashes = SlotHashes::from_account_view(slot_hashes_account)?;
    let recent_entry = slot_hashes
        .get_entry(0)
        .ok_or(ProgramError::InvalidArgument)?;

    let initial_nonce = solana_sha256_hasher::hashv(&[
        // separates this from `Submit`'s derivation, so an initial nonce
        // can never equal an advanced one.
        INIT_NONCE_DERIVATION_TAG,
        // so multiple accounts initialized in the same slot don't share the same nonce
        programmatic_signer_account.address().as_ref(),
        // chain entropy the caller can't choose
        &recent_entry.hash,
    ]);

    let state = ProgrammaticSignerAccount {
        nonce: initial_nonce,
        authority: Address::from(authority.address()),
    };

    // Write data into the account
    let mut data = programmatic_signer_account.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
