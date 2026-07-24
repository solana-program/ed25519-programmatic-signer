use {
    alloc::{collections::BTreeSet, vec::Vec},
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::VersionedMessage,
    solana_sdk_ids::sysvar::slot_hashes,
    spl_message_executor_interface::instruction::Instruction as MessageExecutorInstruction,
};

/// Creates an `Execute` instruction for a wrapped message.
pub fn execute(nonce_account: &Address, message: &VersionedMessage) -> Instruction {
    let account_addrs = message.static_account_keys();

    // Fixed accounts for consuming the nonce, followed by the wrapped message's accounts
    let mut accounts = Vec::with_capacity(account_addrs.len().saturating_add(3));
    accounts.push(AccountMeta::new(*nonce_account, false));
    accounts.push(AccountMeta::new_readonly(spl_nonce_interface::id(), false));
    accounts.push(AccountMeta::new_readonly(slot_hashes::id(), false));

    for (index, address) in account_addrs.iter().enumerate() {
        accounts.push(AccountMeta {
            pubkey: *address,
            is_signer: message.is_signer(index),
            is_writable: message
                .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
        });
    }

    Instruction::new_with_wincode(
        spl_message_executor_interface::id(),
        &MessageExecutorInstruction::Execute(message.clone()),
        accounts,
    )
}
