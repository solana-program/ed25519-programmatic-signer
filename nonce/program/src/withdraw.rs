use {
    pinocchio::{
        AccountView, Address, ProgramResult,
        error::ProgramError,
        sysvars::{Sysvar, clock::Clock, rent::Rent},
    },
    spl_nonce_interface::{error::Error, state::Nonce},
};

/// Withdraws lamports, closing the nonce account when its full balance is withdrawn.
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
    let state = Nonce::view(&data).map_err(|_| Error::InvalidNonceAccount)?;
    if authority.address() != &state.authority {
        return Err(Error::AuthorityMismatch.into());
    }
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let remaining = nonce_account
        .lamports()
        .checked_sub(lamports)
        .ok_or(ProgramError::InsufficientFunds)?;
    if remaining == 0 {
        if Clock::get()?.slot <= state.initialize_slot.into() {
            return Err(Error::CloseSameSlot.into());
        }
    } else if remaining < Rent::get()?.try_minimum_balance(Nonce::LEN)? {
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
