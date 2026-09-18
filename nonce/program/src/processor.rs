use {
    crate::{advance::process_advance, initialize::process_initialize, withdraw::process_withdraw},
    pinocchio::{AccountView, Address, ProgramResult},
    spl_nonce_interface::instruction::Instruction,
};

#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_bytes(instruction_data)? {
        Instruction::Initialize => process_initialize(program_id, accounts),
        Instruction::Advance {
            current_nonce,
            transition_commitment,
        } => process_advance(program_id, accounts, current_nonce, transition_commitment),
        Instruction::Withdraw { lamports } => process_withdraw(program_id, accounts, lamports),
    }
}
