//! Signs a wrapped transaction with [`Signers`] and assembles the on-chain submit instruction.

use {
    crate::{instruction::submit, message::wrapped_message},
    solana_instruction::Instruction,
    solana_signer::{SignerError, signers::Signers},
    solana_transaction::versioned::VersionedTransaction,
};

/// Builds a wrapped transaction for `executor_instruction`, signs it with `signers`, and returns
/// the on-chain signer-program `Submit` instruction.
pub fn sign_and_submit<S: Signers + ?Sized>(
    executor_instruction: &Instruction,
    signers: &S,
) -> Result<Instruction, SignerError> {
    let authorities = signers.try_pubkeys()?;
    let message = wrapped_message(executor_instruction, &authorities);
    let transaction = VersionedTransaction::try_new(message, signers)?;
    let instruction = submit(transaction);
    Ok(instruction)
}
