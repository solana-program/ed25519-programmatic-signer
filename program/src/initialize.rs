use {
    crate::verifier::{SchemeState, SigningScheme},
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, rent::Rent, slot_hashes::SlotHashes},
    },
    spl_ed25519_durable_signer_interface::state::INIT_NONCE_DERIVATION_TAG,
    wincode::{SchemaWrite, config::DefaultConfig},
};

/// Turns a caller-created, program-owned account into a scheme-specific durable
/// signer account with a fresh nonce.
#[inline(never)]
pub fn process_initialize<S: SigningScheme>(
    program_id: &Address,
    accounts: &mut [AccountView],
    initialize: &S::Initialize,
) -> ProgramResult
where
    SchemeState<S>: SchemaWrite<DefaultConfig, Src = SchemeState<S>>,
{
    let [durable_signer_account, remaining_accounts @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let parsed = S::parse_initialize_accounts(remaining_accounts, initialize)?;

    // Caller must ensure account is pre-created with authority set to the program
    if !durable_signer_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    // The account layout is selected at compile time with the signing scheme.
    if durable_signer_account.data_len() != S::STATE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // A fresh account is zero-filled. Any nonzero byte means it is already initialized.
    let data = durable_signer_account.try_borrow()?;
    if data.iter().any(|byte| *byte != 0) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    drop(data);

    let rent_required = Rent::get()?.try_minimum_balance(S::STATE_LEN)?;
    if durable_signer_account.lamports() < rent_required {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Read the most recent slot hash to feed the nonce derivation
    let slot_hashes = SlotHashes::from_account_view(parsed.slot_hashes_account)?;
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

    let state = SchemeState::<S> {
        nonce: initial_nonce,
        authority: parsed.authority,
    };

    // Write data into the account
    let mut data = durable_signer_account.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}
