use {
    crate::{
        Error, Result,
        message::accounts::{is_header_writable, resolve_key},
    },
    solana_address::Address,
    solana_message::{VersionedMessage, compiled_instruction::CompiledInstruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    std::collections::BTreeSet,
};

pub(super) fn verify(
    wrapped_message: &VersionedMessage,
    executor_instruction: &CompiledInstruction,
    inner_message: &VersionedMessage,
    expected_nonce_account: Option<&Address>,
) -> Result<()> {
    let wrapped_keys = wrapped_message.static_account_keys();
    let nonce_account_index = *executor_instruction
        .accounts
        .first()
        .ok_or(Error::InvalidNonceAccountMeta)?;
    if let Some(expected_nonce_account) = expected_nonce_account {
        if resolve_key(wrapped_keys, nonce_account_index)? != expected_nonce_account {
            return Err(Error::InvalidNonceAccountMeta);
        }
    }

    let nonce_program_index = *executor_instruction
        .accounts
        .get(1)
        .ok_or(Error::InvalidNonceProgramId)?;
    if *resolve_key(wrapped_keys, nonce_program_index)? != spl_nonce_interface::id() {
        return Err(Error::InvalidNonceProgramId);
    }

    let slot_hashes_index = *executor_instruction
        .accounts
        .get(2)
        .ok_or(Error::InvalidWrappedTransaction)?;
    if *resolve_key(wrapped_keys, slot_hashes_index)? != slot_hashes::id() {
        return Err(Error::InvalidWrappedTransaction);
    }

    let message_accounts = executor_instruction
        .accounts
        .get(3..)
        .ok_or(Error::InvalidWrappedTransaction)?;
    let message_keys = inner_message.static_account_keys();
    if message_accounts.len() != message_keys.len() {
        return Err(Error::InvalidWrappedTransaction);
    }
    for (message_index, (account_index, message_key)) in
        message_accounts.iter().zip(message_keys).enumerate()
    {
        if resolve_key(wrapped_keys, *account_index)? != message_key {
            return Err(Error::InvalidWrappedTransaction);
        }
        let wrapped_index = usize::from(*account_index);
        if inner_message.is_signer(message_index)
            && !signer_privilege_is_covered(wrapped_message, wrapped_index, message_key)?
        {
            return Err(Error::MissingSignerPrivilege(*message_key));
        }
        if inner_message
            .is_maybe_writable_with_reserved_addresses(message_index, None::<&BTreeSet<_>>)
            && !is_header_writable(wrapped_index, wrapped_message)
        {
            return Err(Error::MissingWritablePrivilege(*message_key));
        }
    }

    Ok(())
}

fn signer_privilege_is_covered(
    wrapped_message: &VersionedMessage,
    wrapped_index: usize,
    inner_signer: &Address,
) -> Result<bool> {
    if wrapped_message.is_signer(wrapped_index) {
        return Ok(true);
    }

    let required_signatures = usize::from(wrapped_message.header().num_required_signatures);
    let signer_keys = wrapped_message
        .static_account_keys()
        .get(..required_signatures)
        .ok_or(Error::InvalidWrappedTransaction)?;
    Ok(signer_keys.iter().any(|authority| {
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), authority)
            == *inner_signer
    }))
}
