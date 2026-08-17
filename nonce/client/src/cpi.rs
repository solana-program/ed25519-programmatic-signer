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
    slot_hashes: &AccountView,
    current_nonce: Hash,
) -> ProgramResult {
    let instruction = Instruction::Advance(AdvanceNonceArgs { current_nonce });
    let data =
        wincode::serialize(&instruction).map_err(|_| ProgramError::InvalidInstructionData)?;

    let instruction_accounts = [
        InstructionAccount::new(authority.address(), false, true),
        InstructionAccount::new(nonce_account.address(), true, false),
        InstructionAccount::new(slot_hashes.address(), false, false),
    ];
    let view = InstructionView {
        program_id: &spl_nonce_interface::ID,
        accounts: instruction_accounts.as_slice(),
        data: data.as_slice(),
    };

    invoke_with_bounds::<3, &AccountView>(&view, &[authority, nonce_account, slot_hashes])
}
