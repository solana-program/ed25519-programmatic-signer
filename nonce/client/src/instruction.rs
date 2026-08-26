use {
    alloc::vec,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_nonce_interface::instruction::Instruction as NonceInstruction,
};

/// Creates an `Initialize` instruction.
pub fn initialize(nonce_account: &Address, authority: &Address) -> Instruction {
    Instruction::new_with_wincode(
        spl_nonce_interface::id(),
        &NonceInstruction::Initialize,
        vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
    )
}

/// Creates an `Advance` instruction.
pub fn advance(
    authority: &Address,
    nonce_account: &Address,
    current_nonce: Hash,
    transition_commitment: Hash,
) -> Instruction {
    Instruction::new_with_wincode(
        spl_nonce_interface::id(),
        &NonceInstruction::Advance {
            current_nonce,
            transition_commitment,
        },
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*nonce_account, false),
        ],
    )
}
