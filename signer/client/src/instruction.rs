//! Builder for the on-chain `Submit` instruction.

use {
    solana_instruction::{AccountMeta, Instruction},
    solana_message::VersionedMessage,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_interface::instruction::Instruction as SignerInstruction,
};

/// Builds the `Submit` instruction from a wrapped transaction envelope.
pub fn submit(transaction: VersionedTransaction) -> Instruction {
    let accounts = transaction
        .message
        .static_account_keys()
        .iter()
        .enumerate()
        .map(|(index, key)| AccountMeta {
            pubkey: *key,
            is_signer: false,
            is_writable: is_message_account_writable(index, &transaction.message),
        })
        .collect();
    Instruction::new_with_wincode(
        spl_ed25519_signer_interface::id(),
        &SignerInstruction::Submit(transaction),
        accounts,
    )
}

// TODO: Replace when no-std version of: https://github.com/anza-xyz/solana-sdk/blob/042f3451979cc8e31a45a09a5627a387ac12a067/message/src/lib.rs#L155-L235
fn is_message_account_writable(index: usize, message: &VersionedMessage) -> bool {
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

    is_writable_index
        && (!message.is_invoked(index)
            || account_keys.contains(&solana_sdk_ids::bpf_loader_upgradeable::id()))
}
