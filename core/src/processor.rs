use {
    crate::{
        instruction::Instruction,
        nonce::{WithdrawAmount, process_initialize_nonce, process_withdraw_nonce},
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
        Instruction::WithdrawNonce { lamports } => {
            process_withdraw_nonce(program_id, accounts, WithdrawAmount::Lamports(lamports))
        }
        Instruction::CloseNonce => {
            process_withdraw_nonce(program_id, accounts, WithdrawAmount::All)
        }
    }
}
