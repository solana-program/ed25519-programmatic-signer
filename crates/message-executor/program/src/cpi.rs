use {
    crate::validate::ReplayAccount,
    alloc::vec::Vec,
    pinocchio::{
        AccountView, ProgramResult,
        cpi::invoke_with_slice,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_message::VersionedMessage,
};

/// Replays each wrapped message instruction as CPI.
pub(crate) fn replay_ixs_via_cpi(
    replay_accounts: &[ReplayAccount],
    wrapped_message: &VersionedMessage,
) -> ProgramResult {
    // The buffers are reused across instructions because the bump allocator never frees
    let mut instruction_accounts = Vec::new();
    let mut account_views = Vec::new();

    for compiled in wrapped_message.instructions() {
        // Infallible: the replay policy rejects address table lookups and sanitization
        // bounds every instruction index by the static account keys, which the bound
        // replay accounts mirror one-to-one.
        let program_account = replay_accounts
            .get(usize::from(compiled.program_id_index))
            .unwrap();

        instruction_accounts.clear();
        account_views.clear();

        for account_index in &compiled.accounts {
            let replay_account = replay_accounts.get(usize::from(*account_index)).unwrap();

            // Metas forward exactly the privileges validation bound. No seeds are
            // involved. Runtime signer privilege propagates through `invoke`.
            instruction_accounts.push(InstructionAccount::new(
                replay_account.account.address(),
                replay_account.is_writable,
                replay_account.is_signer,
            ));
            account_views.push(replay_account.account);
        }

        let view = InstructionView {
            program_id: program_account.account.address(),
            accounts: instruction_accounts.as_slice(),
            data: compiled.data.as_slice(),
        };

        invoke_with_slice::<&AccountView>(&view, account_views.as_slice())?;
    }

    Ok(())
}
