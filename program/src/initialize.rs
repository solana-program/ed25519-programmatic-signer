use {
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent, slot_hashes::SlotHashes},
    },
    spl_ed25519_programmatic_signer_legacy_interface::state::{
        INIT_NONCE_DERIVATION_TAG, SignerContext,
    },
};

/// Turns a caller-created, program-owned account into a [`SignerContext`]
/// bound to `authority` with a fresh nonce.
#[inline(always)]
pub fn process_initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [signer_context, authority, slot_hashes_account, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Caller must ensure account is pre-created with authority set to the program
    if !signer_context.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // Ensure's the precise length matches the layout
    if signer_context.data_len() != SignerContext::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // A fresh account is zero-filled. Any nonzero byte means it is already initialized.
    let data = signer_context.try_borrow()?;
    if data.iter().any(|byte| *byte != 0) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    drop(data);

    let rent_required = Rent::get()?.try_minimum_balance(SignerContext::LEN)?;
    if signer_context.lamports() < rent_required {
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
        signer_context.address().as_ref(),
        // chain entropy the caller can't choose
        &recent_entry.hash,
    ]);

    let state = SignerContext {
        nonce: initial_nonce,
        authority: Address::from(authority.address()),
    };

    // Write data into the account
    let mut data = signer_context.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
