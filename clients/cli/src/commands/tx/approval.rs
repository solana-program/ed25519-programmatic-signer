use {
    anyhow::{Context, Result, bail, ensure},
    solana_address::Address,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy},
    solana_sanitize::Sanitize,
    spl_legacy_message_executor_interface::instruction::Instruction as ExecutorInstruction,
};

/// An approval's outer message, inner message, and nonce account for review.
pub struct ApprovalDetails<'a> {
    pub outer_message: &'a VersionedMessage,
    pub inner_message: legacy::Message,
    pub nonce_account: Address,
}

/// Sanitize both messages, check the outer approval, and verify the Execute account layout.
/// These offline checks do not establish execution validity.
pub fn validate_approval_message(outer_message: &VersionedMessage) -> Result<ApprovalDetails<'_>> {
    outer_message.sanitize().context("invalid outer message")?;

    let outer_account_keys = outer_message.static_account_keys();
    let [execute_instruction] = outer_message.instructions() else {
        bail!("expected exactly one Execute instruction");
    };
    ensure!(
        outer_account_keys.get(usize::from(execute_instruction.program_id_index))
            == Some(&spl_legacy_message_executor_interface::id()),
        "expected the Legacy Message Executor"
    );

    // Keep approval signatures unusable for direct transactions that can charge fees to the authority
    ensure!(
        outer_message.recent_blockhash() == &Hash::default(),
        "outer message must use the default blockhash to prevent native transaction fees"
    );

    ensure!(
        execute_instruction
            .accounts
            .iter()
            .all(|index| usize::from(*index) < outer_account_keys.len()),
        "Execute accounts must use static account keys because ALTs are not resolved"
    );

    let ExecutorInstruction::Execute(inner_message) =
        ExecutorInstruction::try_from_bytes(&execute_instruction.data)
            .context("invalid Execute instruction")?;
    inner_message.sanitize().context("invalid inner message")?;

    // Legacy sanitization permits duplicates, but the executor rejects them.
    ensure!(
        !inner_message.has_duplicates(),
        "inner message must not contain duplicate account keys"
    );

    let [
        nonce_account_index,
        nonce_program_index,
        inner_account_indices @ ..,
    ] = execute_instruction.accounts.as_slice()
    else {
        bail!("expected the nonce account and SPL Nonce program in Execute accounts");
    };
    let nonce_account = outer_account_keys[usize::from(*nonce_account_index)];
    ensure!(
        outer_account_keys[usize::from(*nonce_program_index)] == spl_nonce_interface::id(),
        "expected the SPL Nonce program as the second Execute account"
    );
    ensure!(
        inner_account_indices
            .iter()
            .map(|index| &outer_account_keys[usize::from(*index)])
            .eq(&inner_message.account_keys),
        "Execute accounts must mirror the inner message accounts"
    );

    Ok(ApprovalDetails {
        outer_message,
        inner_message,
        nonce_account,
    })
}
