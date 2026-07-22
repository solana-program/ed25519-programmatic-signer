//! Builder for the on-chain `Submit` instruction.

use {
    alloc::collections::BTreeSet,
    solana_instruction::{AccountMeta, Instruction},
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
            is_writable: transaction
                .message
                .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
        })
        .collect();
    Instruction::new_with_wincode(
        spl_ed25519_signer_interface::id(),
        &SignerInstruction::Submit(transaction),
        accounts,
    )
}
