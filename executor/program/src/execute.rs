use {
    crate::{
        cpi::invoke_instructions,
        validate::{validate_message_accounts, validate_wrapped_message},
    },
    pinocchio::{AccountView, ProgramResult, error::ProgramError},
    solana_message::VersionedMessage,
    solana_sdk_ids::sysvar::slot_hashes as slot_hashes_sysvar_id,
    spl_message_executor_interface::{error::Error, instruction::derive_transition_commitment},
    spl_nonce_interface::state::Nonce,
};

pub fn process_execute(
    accounts: &mut [AccountView],
    wrapped_message: VersionedMessage,
) -> ProgramResult {
    let [
        nonce_account,
        nonce_program,
        slot_hashes,
        message_accounts @ ..,
    ] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if nonce_program.address() != &spl_nonce_interface::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !nonce_account.owned_by(&spl_nonce_interface::ID) {
        return Err(ProgramError::IllegalOwner);
    }
    if slot_hashes.address() != &slot_hashes_sysvar_id::ID {
        return Err(ProgramError::UnsupportedSysvar);
    }

    let Nonce { nonce, authority } = wincode::deserialize_exact(&nonce_account.try_borrow()?)
        .map_err(|_| Error::InvalidNonceAccount)?;

    validate_wrapped_message(&wrapped_message)?;

    if wrapped_message.recent_blockhash() != &nonce {
        return Err(Error::NonceMismatch.into());
    }

    let nonce_authority_account =
        validate_message_accounts(message_accounts, &wrapped_message, &authority)?;

    // Consume the nonce before invoking the message instructions to prevent recursive execution.
    // The advance rolls back if any instruction invocation fails.
    spl_nonce_client::cpi::advance(
        nonce_authority_account,
        nonce_account,
        nonce,
        derive_transition_commitment(&wrapped_message),
    )?;

    invoke_instructions(message_accounts, &wrapped_message)
}
