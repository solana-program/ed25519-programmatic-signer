//! Submit transaction assembly.

use {
    crate::{Error, Result, is_fully_signed},
    solana_address::Address,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy::Message},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_client::instruction::submit,
    spl_message_executor_interface::instruction::Instruction as ExecutorInstruction,
};

/// Builds and signs an outer submit transaction.
pub fn submit_transaction(
    transaction: &VersionedTransaction,
    fee_payer: &dyn Signer,
    extra_signers: &[&dyn Signer],
    recent_blockhash: Hash,
) -> Result<VersionedTransaction> {
    crate::verify::verify_static(transaction)?;
    if !is_fully_signed(transaction) {
        return Err(Error::NotFullySigned);
    }

    let fee_payer_key = fee_payer.try_pubkey()?;
    let required_outer_signers = required_outer_signers(transaction)?;
    let provided_signer_keys = provided_signer_keys(fee_payer_key, extra_signers)?;
    for required_outer_signer in &required_outer_signers {
        if !provided_signer_keys.contains(required_outer_signer) {
            return Err(Error::MissingOuterSigner(*required_outer_signer));
        }
    }

    let mut instruction = submit(transaction.clone());
    let mut signers = Vec::with_capacity(1usize.saturating_add(extra_signers.len()));
    let mut transaction_signer_keys =
        Vec::with_capacity(1usize.saturating_add(extra_signers.len()));
    signers.push(fee_payer);
    transaction_signer_keys.push(fee_payer_key);

    for required_outer_signer in &required_outer_signers {
        mark_outer_signer(&mut instruction, *required_outer_signer)?;
    }

    for extra_signer in extra_signers {
        let extra_signer_key = extra_signer.try_pubkey()?;
        if transaction_signer_keys.contains(&extra_signer_key) {
            continue;
        }
        if !required_outer_signers.contains(&extra_signer_key) {
            return Err(Error::OuterSignerNotRequired(extra_signer_key));
        }
        signers.push(*extra_signer);
        transaction_signer_keys.push(extra_signer_key);
    }

    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[instruction],
        Some(&fee_payer_key),
        &recent_blockhash,
    ));
    VersionedTransaction::try_new(message, signers.as_slice()).map_err(Error::from)
}

fn required_outer_signers(transaction: &VersionedTransaction) -> Result<Vec<Address>> {
    let [executor_instruction] = transaction.message.instructions() else {
        return Err(Error::InvalidExecutorInstructionCount);
    };
    let ExecutorInstruction::Execute(inner_message) =
        ExecutorInstruction::try_from_bytes(&executor_instruction.data)
            .map_err(|_| Error::InvalidInstructionData)?;
    let wrapped_required_signatures =
        usize::from(transaction.message.header().num_required_signatures);
    let inner_required_signatures = usize::from(inner_message.header().num_required_signatures);
    let wrapped_signers = transaction
        .message
        .static_account_keys()
        .get(..wrapped_required_signatures)
        .ok_or(Error::InvalidWrappedTransaction)?;
    let inner_signers = inner_message
        .static_account_keys()
        .get(..inner_required_signatures)
        .ok_or(Error::InvalidInnerMessage)?;

    let mut required_outer_signers = Vec::new();
    for inner_signer in inner_signers {
        if wrapped_signers.contains(inner_signer) && !required_outer_signers.contains(inner_signer)
        {
            required_outer_signers.push(*inner_signer);
        }
    }

    Ok(required_outer_signers)
}

fn provided_signer_keys(
    fee_payer_key: Address,
    extra_signers: &[&dyn Signer],
) -> Result<Vec<Address>> {
    let mut signer_keys = Vec::with_capacity(1usize.saturating_add(extra_signers.len()));
    signer_keys.push(fee_payer_key);
    for extra_signer in extra_signers {
        let extra_key = extra_signer.try_pubkey()?;
        if !signer_keys.contains(&extra_key) {
            signer_keys.push(extra_key);
        }
    }
    Ok(signer_keys)
}

fn mark_outer_signer(
    instruction: &mut solana_instruction::Instruction,
    signer: Address,
) -> Result<()> {
    for meta in &mut instruction.accounts {
        if meta.pubkey == signer {
            meta.is_signer = true;
            return Ok(());
        }
    }

    Err(Error::InvalidWrappedTransaction)
}
