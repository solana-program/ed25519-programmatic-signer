use {
    pinocchio::{
        AccountView, ProgramResult,
        cpi::invoke_with_bounds,
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_hash::Hash,
    spl_nonce_interface::instruction::{AdvanceNonce, Instruction as NonceInstruction},
};

/// Invokes the SPL Nonce program's `Advance` instruction.
pub fn advance(
    nonce_account: &AccountView,
    authority: &AccountView,
    slot_hashes: &AccountView,
    current_nonce: Hash,
) -> ProgramResult {
    let instruction = NonceInstruction::Advance(AdvanceNonce { current_nonce });
    let data =
        wincode::serialize(&instruction).map_err(|_| ProgramError::InvalidInstructionData)?;

    let instruction_accounts = [
        InstructionAccount::new(nonce_account.address(), true, false),
        InstructionAccount::new(authority.address(), false, true),
        InstructionAccount::new(slot_hashes.address(), false, false),
    ];
    let view = InstructionView {
        program_id: &spl_nonce_interface::ID,
        accounts: instruction_accounts.as_slice(),
        data: data.as_slice(),
    };

    invoke_with_bounds::<3, &AccountView>(&view, &[nonce_account, authority, slot_hashes])
}
