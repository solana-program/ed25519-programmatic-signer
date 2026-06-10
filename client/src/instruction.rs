use {
    alloc::vec,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction as SolanaInstruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_ed25519_programmatic_signer_legacy_interface::instruction::Instruction,
};

/// Creates an `Initialize` instruction.
pub fn initialize(signer_context: &Address, authority: &Address) -> SolanaInstruction {
    SolanaInstruction {
        program_id: spl_ed25519_programmatic_signer_legacy_interface::id(),
        accounts: vec![
            AccountMeta::new(*signer_context, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
        data: vec![Instruction::Initialize as u8],
    }
}
