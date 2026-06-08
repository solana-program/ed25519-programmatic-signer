use pinocchio::{
    AccountView, Address, ProgramResult, default_allocator, nostd_panic_handler, program_entrypoint,
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
    crate::processor::process_instruction::<crate::config::ActiveVerifier>(
        program_id,
        accounts,
        instruction_data,
    )
}
