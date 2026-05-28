use {
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent, slot_hashes::SlotHashes},
    },
    spl_ed25519_durable_signer_interface::state::{
        DurableSignerAccount, INIT_NONCE_DERIVATION_TAG,
    },
};

/// Turns a caller-created, program-owned account into a [`DurableSignerAccount`]
/// bound to `authority` with a fresh nonce.
#[inline(always)]
pub fn process_initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [durable_signer_account, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Caller must ensure account is pre-created with authority set to the program
    if !durable_signer_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // Ensure's precise length matches the layout
    if durable_signer_account.data_len() != DurableSignerAccount::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let rent_required = Rent::get()?.try_minimum_balance(DurableSignerAccount::LEN)?;
    if durable_signer_account.lamports() < rent_required {
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
        durable_signer_account.address().as_ref(),
        // chain entropy the caller can't choose
        &recent_entry.hash,
    ]);

    let state = DurableSignerAccount {
        nonce: initial_nonce,
        authority: Address::from(authority.address()),
    };

    // A fresh account is zero-filled. Any nonzero byte means it is already initialized.
    let mut data = durable_signer_account.try_borrow_mut()?;
    if data.iter().any(|byte| *byte != 0) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // Write data into the account
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
