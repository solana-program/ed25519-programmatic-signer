use {
    alloc::vec::Vec,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::VersionedMessage,
    solana_sdk_ids::sysvar::slot_hashes,
    spl_message_executor_interface::{
        instruction::Instruction as MessageExecutorInstruction,
        message::is_message_account_writable,
    },
};

/// Creates an `Execute` instruction for a wrapped message.
///
/// Account metas mirror what the program enforces. The wrapped message accounts are
/// passed in order, with message writability and required signer keys marked.
/// When `Execute` is invoked through a signer program, that program provides signer
/// privilege instead, so callers may clear the signer flags before composing.
pub fn execute(nonce_account: &Address, message: &VersionedMessage) -> Instruction {
    let account_keys = message.static_account_keys();

    // Fixed accounts for consuming the nonce, followed by the wrapped message's accounts
    let mut accounts = Vec::with_capacity(3usize.saturating_add(account_keys.len()));
    accounts.push(AccountMeta::new(*nonce_account, false));
    accounts.push(AccountMeta::new_readonly(spl_nonce_interface::id(), false));
    accounts.push(AccountMeta::new_readonly(slot_hashes::id(), false));
    for (index, key) in account_keys.iter().enumerate() {
        accounts.push(AccountMeta {
            pubkey: *key,
            is_signer: message.is_signer(index),
            is_writable: is_message_account_writable(index, message),
        });
    }

    Instruction::new_with_wincode(
        spl_message_executor_interface::id(),
        &MessageExecutorInstruction::Execute(message.clone()),
        accounts,
    )
}
