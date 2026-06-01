use {
    alloc::vec,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_nonce_interface::instruction::{AdvanceNonce, Instruction as NonceInstruction},
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
pub fn advance(nonce_account: &Address, authority: &Address, current_nonce: Hash) -> Instruction {
    Instruction::new_with_wincode(
        spl_nonce_interface::id(),
        &NonceInstruction::Advance(AdvanceNonce { current_nonce }),
        vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
    )
}
