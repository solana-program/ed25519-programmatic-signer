use {
    crate::{
        error::Error, execute::process_execute, instruction::Instruction,
        nonce::process_initialize_nonce, scheme::SignerScheme,
    },
    pinocchio::{AccountView, Address, ProgramResult},
};

/// Entrypoint processor for an SPL Programmatic Signer program implementing [`SignerScheme`].
#[inline(always)]
pub fn process_instruction<S: SignerScheme>(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::<S::Authorization>::try_from_bytes(instruction_data)? {
        Instruction::InitializeNonce => process_initialize_nonce(program_id, accounts),
        Instruction::Execute {
            signatures,
            header,
            expected_nonce,
            instructions,
        } => process_execute::<S>(
            program_id,
            accounts,
            header,
            expected_nonce,
            instructions,
            &signatures,
        ),
        Instruction::ConsumeNonce => Err(Error::ConsumeNonceNotCallable.into()),
    }
}
