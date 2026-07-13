use {
    crate::submit::process_submit,
    pinocchio::{
        AccountView, Address, ProgramResult, default_allocator, nostd_panic_handler,
        program_entrypoint,
    },
    spl_ed25519_signer_interface::instruction::Instruction,
};

program_entrypoint!(process_instruction);
default_allocator!();
nostd_panic_handler!();

#[inline(always)]
fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_bytes(instruction_data)? {
        Instruction::Submit(transaction) => process_submit(program_id, accounts, transaction),
    }
}
