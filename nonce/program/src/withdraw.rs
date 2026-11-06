use {
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, clock::Clock, rent::Rent},
    },
    spl_nonce_interface::{error::Error, state::Nonce},
};

/// Withdraws lamports, closing the nonce account when its full balance is withdrawn.
///
/// An initialized nonce account requires its stored authority to sign. Any other
/// program-owned account can only be withdrawn from with the nonce account's own signature, so they can be recovered.
#[inline(never)]
pub fn process_withdraw(
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
