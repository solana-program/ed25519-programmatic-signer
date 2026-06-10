//! Signs an executor instruction with [`Signers`] and assembles the on-chain instruction.

use {
    crate::instruction::submit,
    alloc::format,
    solana_instruction::Instruction,
    solana_signer::{SignerError, signers::Signers},
    spl_ed25519_signer_interface::instruction::{SubmitEnvelope, SubmitPayload},
};

/// Builds the payload for `executor_instruction`, signs it with `signers`, and
/// returns the on-chain `Submit` instruction.
pub fn sign_and_submit<S: Signers + ?Sized>(
    executor_instruction: &Instruction,
    signers: &S,
) -> Result<Instruction, SignerError> {
    let payload = SubmitPayload {
        signer_program_id: spl_ed25519_signer_interface::id(),
        executor_program_id: executor_instruction.program_id,
        executor_instruction_data: executor_instruction.data.clone(),
    };
    let message = payload.signing_bytes().map_err(|error| {
        SignerError::Custom(format!("failed to serialize submit payload: {error:?}"))
    })?;
    let signatures = signers.try_sign_message(&message)?;
    let authorities = signers.try_pubkeys()?;
    Ok(submit(
        SubmitEnvelope {
            signatures,
            payload,
        },
        &authorities,
        &executor_instruction.accounts,
    ))
}
