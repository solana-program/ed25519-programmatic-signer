use {
    crate::{Error, Result},
    solana_address::Address,
    solana_message::VersionedMessage,
};

pub(crate) fn resolve_key(keys: &[Address], index: u8) -> Result<&Address> {
    keys.get(usize::from(index))
        .ok_or(Error::InvalidWrappedTransaction)
}

pub(crate) fn required_signers(message: &VersionedMessage) -> Result<&[Address]> {
    let required_signatures = usize::from(message.header().num_required_signatures);
    message
        .static_account_keys()
        .get(..required_signatures)
        .ok_or(Error::InvalidInnerMessage)
}

pub(crate) fn is_header_writable(index: usize, message: &VersionedMessage) -> bool {
    let header = message.header();
    let account_keys = message.static_account_keys();
    let required_signatures = usize::from(header.num_required_signatures);
    let writable_signers_end =
        required_signatures.saturating_sub(usize::from(header.num_readonly_signed_accounts));
    let writable_unsigned_end = account_keys
        .len()
        .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
    index < writable_signers_end || (required_signatures..writable_unsigned_end).contains(&index)
}
