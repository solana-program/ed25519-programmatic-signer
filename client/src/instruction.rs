use {
    alloc::vec,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_ed25519_durable_signer_interface::instruction::DurableSignerInstruction,
};

/// Creates an `Initialize` instruction.
pub fn initialize(durable_signer: &Address, authority: &Address) -> Instruction {
    Instruction {
        program_id: spl_ed25519_durable_signer_interface::id(),
        accounts: vec![
            AccountMeta::new(*durable_signer, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
        data: vec![DurableSignerInstruction::Initialize as u8],
    }
}
