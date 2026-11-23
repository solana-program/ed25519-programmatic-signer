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

/// `core` lowers `intrinsics::abort()` to a call to an `abort` symbol, which only `std` defines.
/// SBPFv3 rejects undefined symbols, so forward it to the `abort` syscall.
///
/// Only defined with static syscalls: otherwise the syscall is itself the `abort` symbol.
#[cfg(target_feature = "static-syscalls")]
#[no_mangle]
extern "C" fn abort() -> ! {
    // SAFETY: The `abort` syscall takes no arguments and never returns.
    unsafe { pinocchio::syscalls::abort() }
}

#[inline(always)]
fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::try_from_bytes(instruction_data)? {
        Instruction::Submit {
            signatures,
            message,
        } => process_submit(program_id, accounts, &signatures, &message),
    }
}
