//! Validates the wrapped message and the runtime accounts used to replay it.

use {
    alloc::collections::BTreeSet,
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_message::{VersionedMessage, v1::TransactionConfig},
    spl_message_executor_interface::error::Error,
};

pub fn validate_wrapped_message(wrapped_message: &VersionedMessage) -> Result<(), ProgramError> {
    // Reject message features that CPI replay does not support. Address table lookups
    // are never resolved and v1 transaction config only applies at transaction load.
    match wrapped_message {
        VersionedMessage::Legacy(_) => {}
        VersionedMessage::V0(v0) if v0.address_table_lookups.is_empty() => {}
        VersionedMessage::V1(v1) if v1.config == TransactionConfig::empty() => {}
        VersionedMessage::V0(_) | VersionedMessage::V1(_) => {
            return Err(Error::InvalidMessage.into());
        }
    }

    // Replay derives account privileges from the header counts,
    // so they must agree with the key list.
    wrapped_message
        .sanitize()
        .map_err(|_| Error::InvalidMessage)?;

    // The runtime's `AccountLoadedTwice` check only covers top-level messages.
    // Reject duplicates so one account cannot hold conflicting CPI privileges.
    if has_duplicate_addresses(wrapped_message.static_account_keys()) {
        return Err(Error::InvalidMessage.into());
    }

    Ok(())
}

fn has_duplicate_addresses(mut addresses: &[Address]) -> bool {
    while let Some((address, remaining)) = addresses.split_first() {
        if remaining.contains(address) {
            return true;
        }
        addresses = remaining;
    }
    false
}

pub fn validate_replay_accounts<'a>(
    replay_accounts: &'a [AccountView],
    wrapped_message: &VersionedMessage,
    stored_nonce_authority: &Address,
) -> Result<&'a AccountView, ProgramError> {
    let expected_addrs = wrapped_message.static_account_keys();

    // Compiled instructions resolve accounts by index, so replay accounts
    // must mirror the message's static addresses one-to-one
    if replay_accounts.len() != expected_addrs.len() {
        return Err(Error::MessageAccountsMismatch.into());
    }

    let mut nonce_authority_account = None;

    for (index, (account, expected_addr)) in replay_accounts.iter().zip(expected_addrs).enumerate()
    {
        if account.address() != expected_addr {
            return Err(Error::MessageAccountsMismatch.into());
        }

        let is_writable =
            wrapped_message.is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>);
        let is_signer = wrapped_message.is_signer(index);

        // CPI cannot escalate privileges, so each account must be received
        // with at least those the message requests
        if is_writable && !account.is_writable() {
            return Err(Error::MessageAccountsMismatch.into());
        }

        if is_signer && !account.is_signer() {
            return Err(Error::MissingRequiredSigner.into());
        }

        // The stored authority must sign the message to authorize the nonce advance
        if is_signer && account.address() == stored_nonce_authority {
            nonce_authority_account = Some(account);
        }
    }

    nonce_authority_account.ok_or(Error::MissingNonceAuthoritySigner.into())
}
