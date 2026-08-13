//! Offline transaction inspection helpers.

use {
    crate::{Error, Result, message::accounts::resolve_key, transaction::signer_status},
    solana_address::Address,
    solana_hash::Hash,
    solana_message::{VersionedMessage, compiled_instruction::CompiledInstruction},
    solana_transaction::versioned::VersionedTransaction,
    spl_message_executor_interface::instruction::Instruction as ExecutorInstruction,
};

/// Required wrapper signer and current signature status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignerStatus {
    /// Signer address.
    pub address: Address,
    /// Whether the signature slot verifies for this transaction.
    pub signed: bool,
}

/// Decoded cold-signed transaction summary for offline display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionSummary {
    /// Genesis hash signed into the cold-signed transaction for cluster binding.
    pub genesis_hash: Hash,
    /// Signer status for the cold-signed transaction.
    pub wrapper_signers: Vec<SignerStatus>,
    /// Executor program id selected by the cold-signed transaction.
    pub executor_program: Address,
    /// Nonce account consumed by the executor instruction.
    pub nonce_account: Address,
    /// Inner transaction message replayed by the executor.
    pub inner_message: VersionedMessage,
    /// Required signer keys in the inner message.
    pub inner_required_signers: Vec<Address>,
    /// Static account keys in the inner message.
    pub inner_account_keys: Vec<Address>,
    /// Compiled instructions in the inner message.
    pub inner_instructions: Vec<CompiledInstruction>,
}

/// Decodes the cold-signed transaction without RPC for signing-device display.
pub fn inspect(transaction: &VersionedTransaction) -> Result<TransactionSummary> {
    crate::verify::verify_static(transaction)?;
    transaction
        .sanitize()
        .map_err(|_| Error::InvalidWrappedTransaction)?;
    let wrapped_message = &transaction.message;
    let [executor_instruction] = wrapped_message.instructions() else {
        return Err(Error::InvalidExecutorInstructionCount);
    };
    let wrapped_keys = wrapped_message.static_account_keys();
    let executor_program = *resolve_key(wrapped_keys, executor_instruction.program_id_index)?;
    if executor_program != spl_message_executor_interface::id() {
        return Err(Error::InvalidExecutorProgramId);
    }
    let nonce_account_index = *executor_instruction
        .accounts
        .first()
        .ok_or(Error::InvalidNonceAccountMeta)?;
    let nonce_account = *resolve_key(wrapped_keys, nonce_account_index)?;

    let ExecutorInstruction::Execute(inner_message) =
        ExecutorInstruction::try_from_bytes(&executor_instruction.data)
            .map_err(|_| Error::InvalidInstructionData)?;
    let required_signatures = usize::from(inner_message.header().num_required_signatures);
    let inner_required_signers = inner_message
        .static_account_keys()
        .get(..required_signatures)
        .ok_or(Error::InvalidInnerMessage)?
        .to_vec();

    Ok(TransactionSummary {
        genesis_hash: *wrapped_message.recent_blockhash(),
        wrapper_signers: signer_status(transaction)
            .into_iter()
            .map(|(address, signed)| SignerStatus { address, signed })
            .collect(),
        executor_program,
        nonce_account,
        inner_account_keys: inner_message.static_account_keys().to_vec(),
        inner_instructions: inner_message.instructions().to_vec(),
        inner_message,
        inner_required_signers,
    })
}
