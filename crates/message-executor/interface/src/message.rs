//! Helpers defining how `Execute` maps a wrapped message onto runtime accounts.
//!
//! The program enforces these rules when binding and replaying accounts. Clients use
//! the same rules to build the account metas an `Execute` instruction requires.

use solana_message::VersionedMessage;

// TODO: Replace when no-std version of: https://github.com/anza-xyz/solana-sdk/blob/042f3451979cc8e31a45a09a5627a387ac12a067/message/src/lib.rs#L155-L235
/// Rebuilds the write access an account carries when the wrapped message is replayed.
pub fn is_message_account_writable(index: usize, message: &VersionedMessage) -> bool {
    // [writable signers | readonly signers | writable unsigned | readonly unsigned]
    let header = message.header();
    let account_keys = message.static_account_keys();
    let required_signatures = usize::from(header.num_required_signatures);
    let writable_signers_end =
        required_signatures.saturating_sub(usize::from(header.num_readonly_signed_accounts));
    let writable_unsigned_end = account_keys
        .len()
        .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
    let is_writable_index = index < writable_signers_end
        || (required_signatures..writable_unsigned_end).contains(&index);

    // Invoked program accounts are demoted to read-only unless the upgradeable loader
    // is present, mirroring the runtime's write lock demotion
    is_writable_index
        && (!message.is_invoked(index)
            || account_keys.contains(&solana_sdk_ids::bpf_loader_upgradeable::id()))
}
