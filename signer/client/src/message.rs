//! Builder for the wrapped transaction message signed by authorities.

use {
    alloc::vec::Vec,
    core::iter,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::{AccountKeys, VersionedMessage, legacy::Message},
};

/// Builds the wrapped legacy message that the authorities sign over the executor instruction.
///
/// ```text
/// account_keys                    privileges
/// ┌──────────────────────────────┬─────────────────┐
/// │ authorities the executor     │ writable signer │
/// │ writes to (at least 1)       │                 │
/// ├──────────────────────────────┼─────────────────┤
/// │ remaining authorities        │ readonly signer │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor writable accounts   │ writable        │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor program id          │ readonly        │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor readonly accounts   │ readonly        │
/// └──────────────────────────────┴─────────────────┘
/// ```
///
/// The executor instruction's original `AccountMeta::is_signer` flags do not grant signer
/// privilege. CPI signer privilege comes only from required outer signers and from
/// `ProgrammaticSigner` PDA promotion.
pub fn wrapped_message(
    executor_instruction: &Instruction,
    authorities: &[Address],
) -> VersionedMessage {
    // Authorities the executor writes to must land in the header's writable-signer range, so
    // they are ordered ahead of the readonly authorities.
    let (writable_authorities, readonly_authorities): (Vec<_>, Vec<_>) =
        authorities.iter().copied().partition(|authority| {
            executor_instruction
                .accounts
                .iter()
                .any(|meta| meta.is_writable && meta.pubkey == *authority)
        });

    // Every message version requires at least one writable signer, the fee payer.
    // A wrapped message pays no fees, so when the executor writes to no authority the first
    // one carries the writable flag anyway. The over-grant is benign. The flag does not grant
    // CPI signer privilege and a writable account without signer privilege accepts nothing
    // beyond lamport credits.
    let writable_signers_count = writable_authorities.len().max(1);

    let mut writable_unsigned = Vec::new();
    let mut readonly_unsigned = Vec::new();

    // Authorities and the executor program id already have dedicated slots, so only the
    // remaining executor accounts need unsigned slots.
    for meta in &executor_instruction.accounts {
        if authorities.contains(&meta.pubkey) {
            continue;
        }
        if meta.pubkey == executor_instruction.program_id {
            continue;
        }
        if meta.is_writable {
            writable_unsigned.push(meta.pubkey);
        } else {
            readonly_unsigned.push(meta.pubkey);
        }
    }

    // The readonly unsigned range is a suffix, so the invoked executor program id sits ahead
    // of the readonly accounts and is counted with them.
    let account_keys = [writable_authorities, readonly_authorities]
        .concat()
        .iter()
        .copied()
        .chain(writable_unsigned.iter().copied())
        .chain(iter::once(executor_instruction.program_id))
        .chain(readonly_unsigned.iter().copied())
        .collect::<Vec<_>>();

    // Compiling rewrites the program id and account pubkeys as u8 indexes into `account_keys`
    let compiled_instructions = AccountKeys::new(&account_keys, None)
        .try_compile_instructions(core::slice::from_ref(executor_instruction))
        .unwrap();

    let message = Message::new_with_compiled_instructions(
        authorities.len() as u8,
        authorities.len().saturating_sub(writable_signers_count) as u8,
        readonly_unsigned.len().saturating_add(1) as u8,
        account_keys,
        Hash::default(),
        compiled_instructions,
    );

    VersionedMessage::Legacy(message)
}
