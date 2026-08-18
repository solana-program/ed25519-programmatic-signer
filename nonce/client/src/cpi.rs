use {
    pinocchio::{
        AccountView, ProgramResult,
        cpi::invoke_with_bounds,
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_hash::Hash,
    spl_nonce_interface::instruction::{AdvanceNonceArgs, Instruction},
};

/// Invokes the SPL Nonce program's `Advance` instruction.
pub fn advance(
    authority: &AccountView,
    nonce_account: &AccountView,
    current_nonce: Hash,
    transition_commitment: Hash,
) -> ProgramResult {
    let instruction = Instruction::Advance(AdvanceNonceArgs {
        current_nonce,
        transition_commitment,
    });
    let data =
        wincode::serialize(&instruction).map_err(|_| ProgramError::InvalidInstructionData)?;

    let instruction_accounts = [
        InstructionAccount::new(authority.address(), false, true),
        InstructionAccount::new(nonce_account.address(), true, false),
    ];
    let view = InstructionView {
        program_id: &spl_nonce_interface::ID,
        accounts: instruction_accounts.as_slice(),
        data: data.as_slice(),
    };

    invoke_with_bounds::<2, &AccountView>(&view, &[authority, nonce_account])
}
