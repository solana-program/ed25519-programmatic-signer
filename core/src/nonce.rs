use {
    crate::{error::Error, state::Nonce},
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

/// Withdraws lamports, closing the nonce account when its full balance is withdrawn.
///
/// An initialized nonce account requires its stored authority to sign. Any other
/// program-owned account can only be withdrawn from with the nonce account's own signature, so they can be recovered.
#[inline(never)]
pub fn process_withdraw_nonce(
    program_id: &Address,
    accounts: &mut [AccountView],
    lamports: u64,
) -> ProgramResult {
    let [authority, nonce_account, destination, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !nonce_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }
    if destination.address() == nonce_account.address() {
        return Err(ProgramError::InvalidArgument);
    }
    let data = nonce_account.try_borrow()?;
    // Slot the account was initialized at, or `None` if it was never initialized.
    let initialize_slot = match Nonce::view(&data) {
        Ok(state) => {
            if authority.address() != &state.authority {
                return Err(Error::AuthorityMismatch.into());
            }
            Some(u64::from(state.initialize_slot))
        }
        Err(_) => {
            if authority.address() != nonce_account.address() {
                return Err(Error::InvalidNonceAccount.into());
            }
            None
        }
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let remaining = nonce_account
        .lamports()
        .checked_sub(lamports)
        .ok_or(ProgramError::InsufficientFunds)?;
    if remaining == 0 {
        // Do not allow closing an initialized nonce account in the same slot
        // that it was initialized. This would allow re-initializing it with
        // the same value.
        if let Some(initialize_slot) = initialize_slot {
            if Clock::get()?.slot <= initialize_slot {
                return Err(Error::CloseSameSlot.into());
            }
        }
    } else if remaining < Rent::get()?.try_minimum_balance(data.len())? {
        return Err(ProgramError::AccountNotRentExempt);
    }
    drop(data);

    let new_destination_balance = destination
        .lamports()
        .checked_add(lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    destination.set_lamports(new_destination_balance);
    if remaining == 0 {
        nonce_account.close()?;
    } else {
        nonce_account.set_lamports(remaining);
    }
    Ok(())
}
