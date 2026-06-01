//! Validates the wrapped message and the runtime accounts used to replay it.

use {
    alloc::vec::Vec,
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_hash::Hash,
    solana_message::{VersionedMessage, v1::TransactionConfig},
    spl_message_executor_interface::{error::Error, message::is_message_account_writable},
};

/// Validates the wrapped message against the stored nonce and the replay policy.
pub(crate) fn validate_wrapped_message(
    wrapped_message: &VersionedMessage,
    stored_nonce: &Hash,
) -> Result<(), ProgramError> {
    // The `recent_blockhash` field carries the nonce. A mismatch means the message was
    // prepared against another nonce account or its nonce was already consumed
    if wrapped_message.recent_blockhash() != stored_nonce {
        return Err(Error::NonceMismatch.into());
    }

    // Only message shapes that CPI can replay faithfully. Address table lookups are never
    // resolved and v1 transaction config only applies at transaction load, so both are
    // rejected, as is any unknown future message version.
    match wrapped_message {
        VersionedMessage::Legacy(_) => {}
        VersionedMessage::V0(v0) if v0.address_table_lookups.is_empty() => {}
        VersionedMessage::V1(v1) if v1.config == TransactionConfig::empty() => {}
        _ => return Err(Error::InvalidMessage.into()),
    }

    // Sanitize header counts and instruction indexes
    wrapped_message
        .sanitize()
        .map_err(|_| Error::InvalidMessage)?;
    if has_duplicate_account_keys(wrapped_message.static_account_keys()) {
        return Err(Error::InvalidMessage.into());
    }

    Ok(())
}

fn has_duplicate_account_keys(account_keys: &[Address]) -> bool {
    for (index, account_key) in account_keys.iter().enumerate() {
        if account_keys[index.saturating_add(1)..]
            .iter()
            .any(|candidate| candidate == account_key)
        {
            return true;
        }
    }
    false
}

/// A wrapped message account bound to its runtime account, carrying the privileges its
/// replay CPI metas forward.
pub(crate) struct ReplayAccount<'a> {
    pub account: &'a AccountView,
    pub is_writable: bool,
    pub is_signer: bool,
}

/// Validates the account list supplied for replay against the wrapped message's keys and
/// verifies the message's signers are authorized.
pub(crate) fn validate_replay_accounts<'a>(
    replay_accounts: &'a [AccountView],
    wrapped_message: &VersionedMessage,
    stored_nonce_authority: &Address,
) -> Result<(Vec<ReplayAccount<'a>>, &'a AccountView), ProgramError> {
    // The caller must supply the wrapped message's account keys in order, so instruction
    // indexes resolve to the accounts the message names.
    let expected_keys = wrapped_message.static_account_keys();
    if replay_accounts.len() != expected_keys.len() {
        return Err(Error::MessageAccountsMismatch.into());
    }

    let mut bound_accounts = Vec::with_capacity(replay_accounts.len());
    let mut nonce_authority_account = None;
    for (account_index, (replay_account, expected_key)) in
        replay_accounts.iter().zip(expected_keys).enumerate()
    {
        if replay_account.address() != expected_key {
            return Err(Error::MessageAccountsMismatch.into());
        }
        // Replay forwards the message's writability, so the runtime flag must cover it
        let is_writable = is_message_account_writable(account_index, wrapped_message);
        if is_writable && !replay_account.is_writable() {
            return Err(Error::MessageAccountsMismatch.into());
        }
        // This program never verifies signatures. Required signers must already carry
        // privilege from the outer transaction or a signer program's promotion.
        let is_signer = wrapped_message.is_signer(account_index);
        if is_signer {
            if !replay_account.is_signer() {
                return Err(Error::MissingRequiredSigner.into());
            }
            if replay_account.address() == stored_nonce_authority {
                nonce_authority_account = Some(replay_account);
            }
        }
        bound_accounts.push(ReplayAccount {
            account: replay_account,
            is_writable,
            is_signer,
        });
    }

    // The nonce is public account data, so the nonce match alone proves nothing about
    // intent. The stored authority signing the wrapped message proves the account owner
    // authorized consuming it. `Advance` enforces the same privilege again.
    let nonce_authority_account = nonce_authority_account.ok_or(Error::AuthorityMismatch)?;

    Ok((bound_accounts, nonce_authority_account))
}
