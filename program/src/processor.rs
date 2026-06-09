use {
    crate::initialize::process_initialize,
    pinocchio::{AccountView, Address, ProgramResult},
    spl_ed25519_programmatic_signer_interface::instruction::ProgrammaticSignerInstruction,
};

#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match ProgrammaticSignerInstruction::try_from_bytes(instruction_data)? {
        ProgrammaticSignerInstruction::Initialize => process_initialize(program_id, accounts),
        ProgrammaticSignerInstruction::Submit | ProgrammaticSignerInstruction::Close => {
            unimplemented!()
        }
    }
}
