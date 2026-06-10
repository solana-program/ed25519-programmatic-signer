use {
    alloc::vec,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_sdk_ids::sysvar::slot_hashes,
    spl_ed25519_programmatic_signer_interface::instruction::ProgrammaticSignerInstruction,
};

/// Creates an `Initialize` instruction.
pub fn initialize(programmatic_signer_account: &Address, authority: &Address) -> Instruction {
    Instruction {
        program_id: spl_ed25519_programmatic_signer_interface::id(),
        accounts: vec![
            AccountMeta::new(*programmatic_signer_account, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
        data: vec![ProgrammaticSignerInstruction::Initialize as u8],
    }
}
