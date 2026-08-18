use {
    mollusk_svm::{
        Mollusk,
        program::{Builtin, create_keyed_account_for_builtin_program},
    },
    solana_account::Account,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction, error::InstructionError},
    solana_program_runtime::solana_sbpf::program::BuiltinFunctionDefinition,
};

const NAME: &str = "stub_executor";

// Invokes the serialized inner instruction after the discriminator. Native CPI constrains its
// requested privileges to those received from the signer program.
solana_program_runtime::declare_process_instruction!(StubExecutor, 0, |invoke_context| {
    let instruction = {
        let instruction_context = invoke_context
            .transaction_context
            .get_current_instruction_context()?;
        let data = instruction_context
            .get_instruction_data()
            .get(1..)
            .ok_or(InstructionError::InvalidInstructionData)?;
        wincode::deserialize_exact(data).map_err(|_| InstructionError::InvalidInstructionData)?
    };

    invoke_context.native_invoke_signed(instruction, &[])
});

pub fn install(mollusk: &mut Mollusk) {
    mollusk.program_cache.add_builtin(Builtin {
        program_id: spl_message_executor_interface::id(),
        name: NAME,
        register_fn: StubExecutor::register,
    });
}

/// Wraps an inner instruction in the stub format: discriminator, instruction, and program account.
pub fn wrap(instruction: Instruction) -> Instruction {
    let mut data = vec![0];
    data.extend(wincode::serialize(&instruction).unwrap());

    let mut accounts = instruction.accounts;
    accounts.push(AccountMeta::new_readonly(instruction.program_id, false));

    Instruction {
        program_id: spl_message_executor_interface::id(),
        accounts,
        data,
    }
}

pub fn keyed_account() -> (Address, Account) {
    create_keyed_account_for_builtin_program(&spl_message_executor_interface::id(), NAME)
}
