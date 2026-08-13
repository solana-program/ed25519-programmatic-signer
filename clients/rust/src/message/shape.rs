use {
    crate::{Error, Result, message::accounts::required_signers},
    solana_address::Address,
    solana_message::{VersionedMessage, v1::TransactionConfig},
    spl_nonce_interface::state::Nonce,
};

pub(crate) fn validate_inner_message_shape(message: &VersionedMessage) -> Result<()> {
    match message {
        VersionedMessage::Legacy(_) => {}
        VersionedMessage::V0(v0) if v0.address_table_lookups.is_empty() => {}
        VersionedMessage::V0(_) => return Err(Error::AddressLookupTablesUnsupported),
        VersionedMessage::V1(v1) if v1.config == TransactionConfig::empty() => {}
        _ => return Err(Error::InvalidInnerMessage),
    }
    message.sanitize().map_err(|_| Error::InvalidInnerMessage)?;

    if let Some(duplicate) = duplicate_key(message.static_account_keys()) {
        return Err(Error::DuplicateMessageAccount(duplicate));
    }

    Ok(())
}

pub(crate) fn validate_inner_message_nonce(
    message: &VersionedMessage,
    expected_nonce: &Nonce,
) -> Result<()> {
    if message.recent_blockhash() != &expected_nonce.nonce {
        return Err(Error::NonceMismatch);
    }
    validate_inner_message_shape(message)?;

    if !required_signers(message)?.contains(&expected_nonce.authority) {
        return Err(Error::MissingNonceAuthority);
    }

    Ok(())
}

fn duplicate_key(keys: &[Address]) -> Option<Address> {
    for (index, key) in keys.iter().enumerate() {
        if keys[index.saturating_add(1)..]
            .iter()
            .any(|candidate| candidate == key)
        {
            return Some(*key);
        }
    }
    None
}
