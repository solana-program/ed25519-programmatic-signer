use {
    crate::initialize::process_initialize,
    pinocchio::{AccountView, Address, ProgramResult},
    spl_ed25519_durable_signer_interface::instruction::DurableSignerInstruction,
};

#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match DurableSignerInstruction::try_from_bytes(instruction_data)? {
        DurableSignerInstruction::Initialize => process_initialize(program_id, accounts),
        DurableSignerInstruction::Submit | DurableSignerInstruction::Close => unimplemented!(),
    }
}
