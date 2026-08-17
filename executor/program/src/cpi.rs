use {
    alloc::{collections::BTreeSet, vec::Vec},
    pinocchio::{
        AccountView, ProgramResult,
        cpi::invoke_with_slice,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_message::VersionedMessage,
    spl_message_executor_interface::error::Error,
};

pub fn invoke_instructions(
    replay_accounts: &[AccountView],
    wrapped_message: &VersionedMessage,
) -> ProgramResult {
    // Allocate once for the largest instruction and reuse across the replay
    let max_instruction_accounts = wrapped_message
        .instructions()
        .iter()
        .map(|ix| ix.accounts.len())
        .max()
        .unwrap_or_default();
    let mut instruction_accounts = Vec::with_capacity(max_instruction_accounts);
    let mut account_views = Vec::with_capacity(max_instruction_accounts);

    // Replay instructions via CPI
    for ix in wrapped_message.instructions() {
        instruction_accounts.clear();
        account_views.clear();

        // Resolve the invoked program
        let program_account = replay_accounts
            .get(usize::from(ix.program_id_index))
            .ok_or(Error::InvalidMessage)?;

        // Rebuild CPI accounts with exactly the message requested privileges
        for idx in &ix.accounts {
            let account_index = usize::from(*idx);
            let replay_account = replay_accounts
                .get(account_index)
                .ok_or(Error::InvalidMessage)?;
            let is_writable = wrapped_message
                .is_maybe_writable_with_reserved_addresses(account_index, None::<&BTreeSet<_>>);
            let is_signer = wrapped_message.is_signer(account_index);

            instruction_accounts.push(InstructionAccount::new(
                replay_account.address(),
                is_writable,
                is_signer,
            ));
            account_views.push(replay_account);
        }

        let ix_view = InstructionView {
            program_id: program_account.address(),
            accounts: instruction_accounts.as_slice(),
            data: ix.data.as_slice(),
        };

        invoke_with_slice::<&AccountView>(&ix_view, account_views.as_slice())?;
    }

    Ok(())
}
