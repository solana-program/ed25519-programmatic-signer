use {
    crate::{
        instruction::Instruction,
        nonce::{process_advance_nonce, process_initialize_nonce},
    },
    pinocchio::{AccountView, Address, ProgramResult},
};

/// Entrypoint processor for an SPL Programmatic Signer program.
#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_bytes(instruction_data)? {
        Instruction::InitializeNonce => process_initialize_nonce(program_id, accounts),
        Instruction::AdvanceNonce {
            current_nonce,
            salt,
        } => process_advance_nonce(program_id, accounts, current_nonce, salt),
    }
}
