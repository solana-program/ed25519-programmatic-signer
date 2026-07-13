use {
    crate::initialize::process_initialize,
    pinocchio::{AccountView, Address, ProgramResult},
    spl_ed25519_programmatic_signer_legacy_interface::instruction::Instruction,
};

#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_bytes(instruction_data)? {
        Instruction::Initialize => process_initialize(program_id, accounts),
        Instruction::Submit | Instruction::Close => unimplemented!(),
    }
}
