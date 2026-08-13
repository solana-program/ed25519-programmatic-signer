//! Offline cold-signed transaction preflight checks.

mod executor_accounts;

use {
    crate::{
        Error, Result,
        message::{
            accounts::resolve_key, validate_inner_message_nonce, validate_inner_message_shape,
        },
    },
    solana_address::Address,
    solana_hash::Hash,
    solana_transaction::versioned::VersionedTransaction,
    spl_nonce_interface::state::Nonce,
};

/// Verifies static wrapped-transaction invariants against a nonce account snapshot.
pub fn verify(
    transaction: &VersionedTransaction,
    expected: &Nonce,
    nonce_account: &Address,
    expected_genesis_hash: &Hash,
) -> Result<()> {
    verify_static(transaction)?;
    verify_genesis_hash(transaction, expected_genesis_hash)?;

    let wrapped_message = &transaction.message;
    let [executor_instruction] = wrapped_message.instructions() else {
        return Err(Error::InvalidExecutorInstructionCount);
    };
    let spl_message_executor_interface::instruction::Instruction::Execute(inner_message) =
        spl_message_executor_interface::instruction::Instruction::try_from_bytes(
            &executor_instruction.data,
        )
        .map_err(|_| Error::InvalidInstructionData)?;

    validate_inner_message_nonce(&inner_message, expected)?;
    executor_accounts::verify(
        wrapped_message,
        executor_instruction,
        &inner_message,
        Some(nonce_account),
    )
}

/// Verifies the cold-signed transaction is signed for the expected cluster genesis hash.
pub fn verify_genesis_hash(
    transaction: &VersionedTransaction,
    expected_genesis_hash: &Hash,
) -> Result<()> {
    if transaction.message.recent_blockhash() != expected_genesis_hash {
        return Err(Error::GenesisHashMismatch);
    }
    Ok(())
}

/// Verifies wrapped-transaction invariants that do not require an RPC nonce account snapshot.
pub fn verify_static(transaction: &VersionedTransaction) -> Result<()> {
    transaction
        .sanitize()
        .map_err(|_| Error::InvalidWrappedTransaction)?;

    let wrapped_message = &transaction.message;
    let [executor_instruction] = wrapped_message.instructions() else {
        return Err(Error::InvalidExecutorInstructionCount);
    };
    let wrapped_keys = wrapped_message.static_account_keys();
    if *resolve_key(wrapped_keys, executor_instruction.program_id_index)?
        != spl_message_executor_interface::id()
    {
        return Err(Error::InvalidExecutorProgramId);
    }
    let spl_message_executor_interface::instruction::Instruction::Execute(inner_message) =
        spl_message_executor_interface::instruction::Instruction::try_from_bytes(
            &executor_instruction.data,
        )
        .map_err(|_| Error::InvalidInstructionData)?;

    validate_inner_message_shape(&inner_message)?;
    executor_accounts::verify(wrapped_message, executor_instruction, &inner_message, None)
}
