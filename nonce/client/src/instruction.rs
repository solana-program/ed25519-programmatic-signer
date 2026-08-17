use {
    alloc::vec,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_sdk_ids::sysvar::slot_hashes,
    solana_system_interface::instruction as system_instruction,
    spl_nonce_interface::{
        instruction::{AdvanceNonceArgs, Instruction as NonceInstruction},
        state::Nonce,
    },
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

/// Creates the System Program and SPL Nonce instructions required to create and initialize a
/// nonce account.
pub fn create_account(
    payer: &Address,
    nonce_account: &Address,
    authority: &Address,
    rent_lamports: u64,
) -> [Instruction; 2] {
    [
        system_instruction::create_account(
            payer,
            nonce_account,
            rent_lamports,
            Nonce::LEN as u64,
            &spl_nonce_interface::id(),
        ),
        initialize(nonce_account, authority),
    ]
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
