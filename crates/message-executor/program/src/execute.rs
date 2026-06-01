use {
    crate::{
        cpi::replay_ixs_via_cpi,
        validate::{validate_replay_accounts, validate_wrapped_message},
    },
    pinocchio::{AccountView, ProgramResult, error::ProgramError},
    solana_message::VersionedMessage,
    solana_sdk_ids::sysvar::slot_hashes as slot_hashes_sysvar_id,
    spl_message_executor_interface::error::Error,
    spl_nonce_interface::state::Nonce,
};

/// Replays a wrapped message's instructions by CPI, then consumes the nonce.
#[inline(never)]
pub(crate) fn process_execute(
    accounts: &mut [AccountView],
    wrapped_message: VersionedMessage,
) -> ProgramResult {
    let [
        nonce_account,
        nonce_program,
        slot_hashes,
        replay_accounts @ ..,
    ] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    // The fixed accounts serve the final `Advance` CPI. The owner check guarantees the
    // state read below is data the SPL Nonce program wrote.
    if nonce_program.address() != &spl_nonce_interface::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !nonce_account.owned_by(&spl_nonce_interface::ID) {
        return Err(ProgramError::IllegalOwner);
    }
    if slot_hashes.address() != &slot_hashes_sysvar_id::ID {
        return Err(ProgramError::InvalidArgument);
    }

    // The borrow is a temporary so it releases before `Advance` re-borrows the account
    let Nonce { nonce, authority } = wincode::deserialize_exact(&nonce_account.try_borrow()?)
        .map_err(|_| Error::InvalidNonceAccount)?;

    validate_wrapped_message(&wrapped_message, &nonce)?;

    let (replay_accounts, nonce_authority_account) =
        validate_replay_accounts(replay_accounts, &wrapped_message, &authority)?;

    replay_ixs_via_cpi(&replay_accounts, &wrapped_message)?;

    // Consuming the nonce is the final step. `Advance` re-checks the presented nonce, so
    // it cannot be consumed twice within one transaction, even by a replayed instruction.
    spl_nonce_client::cpi::advance(nonce_account, nonce_authority_account, slot_hashes, nonce)
}
