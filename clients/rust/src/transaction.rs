//! Programmatic signer transaction construction, signing, and merging.

use {
    crate::{
        Error, Result, TransactionPlan,
        message::{
            accounts::required_signers, build_inner_message, validate_inner_message_nonce,
            validate_inner_message_shape,
        },
    },
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::VersionedMessage,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_client::message::wrapped_message,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    spl_message_executor_client::instruction::execute,
    spl_nonce_interface::state::Nonce,
};

/// Builds an unsigned cold-signed transaction for a transaction plan and nonce value.
pub fn build_transaction(
    transaction_plan: &TransactionPlan,
    nonce_value: Hash,
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    let inner_message = build_inner_message(transaction_plan, nonce_value)?;
    transaction_from_message(
        inner_message,
        transaction_plan.nonce_account,
        &transaction_plan.authorities,
        &transaction_plan.submit_signers,
        genesis_hash,
    )
}

/// Builds an unsigned cold-signed transaction from an already compiled Solana message.
pub fn transaction_from_message(
    inner_message: VersionedMessage,
    nonce_account: Address,
    authorities: &[Address],
    submit_signers: &[Address],
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    validate_inner_message_shape(&inner_message)?;
    validate_wrapped_signers(&inner_message, authorities, submit_signers)?;

    let executor_instruction = execute(&nonce_account, &inner_message);
    let mut wrapper_signers =
        Vec::with_capacity(authorities.len().saturating_add(submit_signers.len()));
    wrapper_signers.extend_from_slice(authorities);
    wrapper_signers.extend_from_slice(submit_signers);
    validate_wrapped_message_bounds(&executor_instruction, &wrapper_signers)?;
    let mut message = wrapped_message(&executor_instruction, &wrapper_signers);
    message.set_recent_blockhash(genesis_hash);
    unsigned_transaction(message)
}

/// Builds an unsigned cold-signed transaction from a compiled message and a nonce snapshot.
pub fn transaction_from_message_checked(
    inner_message: VersionedMessage,
    nonce_account: Address,
    expected_nonce: &Nonce,
    authorities: &[Address],
    submit_signers: &[Address],
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    validate_inner_message_nonce(&inner_message, expected_nonce)?;
    transaction_from_message(
        inner_message,
        nonce_account,
        authorities,
        submit_signers,
        genesis_hash,
    )
}

/// Builds an unsigned cold-signed transaction from Solana-style sign-only JSON.
pub fn transaction_from_sign_only(
    sign_only: &crate::sign_only::SignOnlyTransaction,
    nonce_account: Address,
    authorities: &[Address],
    submit_signers: &[Address],
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    transaction_from_message(
        sign_only.message()?,
        nonce_account,
        authorities,
        submit_signers,
        genesis_hash,
    )
}

/// Builds an unsigned cold-signed transaction from Solana-style sign-only JSON and a nonce snapshot.
pub fn transaction_from_sign_only_checked(
    sign_only: &crate::sign_only::SignOnlyTransaction,
    nonce_account: Address,
    expected_nonce: &Nonce,
    authorities: &[Address],
    submit_signers: &[Address],
    genesis_hash: Hash,
) -> Result<VersionedTransaction> {
    transaction_from_message_checked(
        sign_only.message()?,
        nonce_account,
        expected_nonce,
        authorities,
        submit_signers,
        genesis_hash,
    )
}

/// Adds one signer signature to the cold-signed transaction.
pub fn sign_transaction(transaction: &mut VersionedTransaction, signer: &dyn Signer) -> Result<()> {
    crate::verify::verify_static(transaction)?;
    let signer_key = signer.try_pubkey()?;
    ensure_signature_slots(transaction)?;

    let required_signatures = usize::from(transaction.message.header().num_required_signatures);
    let signer_keys = transaction
        .message
        .static_account_keys()
        .get(..required_signatures)
        .ok_or(Error::InvalidWrappedTransaction)?;
    if !signer_keys.contains(&signer_key) {
        return Err(Error::SignerNotRequired(signer_key));
    }

    let signature = signer.try_sign_message(&transaction.message.serialize())?;
    for (index, key) in signer_keys.iter().enumerate() {
        if *key == signer_key {
            transaction.signatures[index] = signature;
        }
    }
    Ok(())
}

/// Merges signatures from another copy of the same cold-signed transaction.
pub fn merge_transactions(
    transaction: &mut VersionedTransaction,
    other: &VersionedTransaction,
) -> Result<()> {
    if transaction.message != other.message {
        return Err(Error::TransactionMismatch);
    }
    ensure_signature_slots(transaction)?;
    ensure_signature_slots(other)?;

    let message_bytes = transaction.message.serialize();
    for (index, other_signature) in other.signatures.iter().enumerate() {
        if !signature_is_present(other_signature) {
            continue;
        }

        let signer = transaction
            .message
            .static_account_keys()
            .get(index)
            .copied()
            .ok_or(Error::InvalidWrappedTransaction)?;
        if !other_signature.verify(signer.as_ref(), &message_bytes) {
            return Err(Error::InvalidSignature(signer));
        }

        let self_signature = transaction
            .signatures
            .get_mut(index)
            .ok_or(Error::InvalidWrappedTransaction)?;
        if signature_is_present(self_signature) && self_signature != other_signature {
            return Err(Error::SignatureConflict(signer));
        }
        *self_signature = *other_signature;
    }

    Ok(())
}

/// Returns each required signer and whether its signature slot is populated.
pub fn signer_status(transaction: &VersionedTransaction) -> Vec<(Address, bool)> {
    let required_signatures = usize::from(transaction.message.header().num_required_signatures);
    let signature_statuses = transaction
        .sanitize()
        .map(|()| transaction.verify_with_results())
        .unwrap_or_default();
    transaction
        .message
        .static_account_keys()
        .iter()
        .take(required_signatures)
        .enumerate()
        .map(|(index, key)| {
            let is_signed = signature_statuses.get(index).copied().unwrap_or(false);
            (*key, is_signed)
        })
        .collect()
}

/// Returns true when every required signature slot is populated.
pub fn is_fully_signed(transaction: &VersionedTransaction) -> bool {
    signer_status(transaction)
        .iter()
        .all(|(_address, signed)| *signed)
}

pub(crate) fn unsigned_transaction(message: VersionedMessage) -> Result<VersionedTransaction> {
    let required_signatures = usize::from(message.header().num_required_signatures);
    if required_signatures > message.static_account_keys().len() {
        return Err(Error::InvalidWrappedTransaction);
    }

    Ok(VersionedTransaction {
        signatures: core::iter::repeat_with(Default::default)
            .take(required_signatures)
            .collect(),
        message,
    })
}

pub(crate) fn ensure_signature_slots(transaction: &VersionedTransaction) -> Result<()> {
    let required_signatures = usize::from(transaction.message.header().num_required_signatures);
    if transaction.signatures.len() != required_signatures
        || required_signatures > transaction.message.static_account_keys().len()
    {
        return Err(Error::InvalidWrappedTransaction);
    }
    Ok(())
}

fn validate_wrapped_signers(
    inner_message: &VersionedMessage,
    authorities: &[Address],
    submit_signers: &[Address],
) -> Result<()> {
    if authorities.is_empty() {
        return Err(Error::EmptyAuthorities);
    }

    let mut wrapper_signers =
        Vec::with_capacity(authorities.len().saturating_add(submit_signers.len()));
    for signer in authorities.iter().chain(submit_signers) {
        if wrapper_signers.contains(signer) {
            return Err(Error::DuplicateAddress(*signer));
        }
        wrapper_signers.push(*signer);
    }

    let inner_signers = required_signers(inner_message)?;
    for submit_signer in submit_signers {
        if !inner_signers.contains(submit_signer) {
            return Err(Error::SubmitSignerNotRequired(*submit_signer));
        }
        if authorities.iter().any(|authority| {
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), authority)
                == *submit_signer
        }) {
            return Err(Error::SubmitSignerCannotBeProgrammaticSigner(
                *submit_signer,
            ));
        }
    }
    for inner_signer in inner_signers {
        let is_submit_signer = submit_signers.contains(inner_signer);
        let is_programmatic_signer = authorities.iter().any(|authority| {
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), authority)
                == *inner_signer
        });
        if !is_submit_signer && !is_programmatic_signer {
            return Err(Error::RequiredInnerSignerNotCovered(*inner_signer));
        }
    }

    Ok(())
}

fn validate_wrapped_message_bounds(
    executor_instruction: &Instruction,
    wrapper_signers: &[Address],
) -> Result<()> {
    if wrapper_signers.len() > usize::from(u8::MAX) {
        return Err(Error::TooManySigners(wrapper_signers.len()));
    }

    let mut account_keys_len = wrapper_signers.len();
    let mut readonly_unsigned_count = 1usize; // the executor program id
    for meta in &executor_instruction.accounts {
        if wrapper_signers.contains(&meta.pubkey) || meta.pubkey == executor_instruction.program_id
        {
            continue;
        }
        account_keys_len = account_keys_len.saturating_add(1);
        if !meta.is_writable {
            readonly_unsigned_count = readonly_unsigned_count.saturating_add(1);
        }
    }
    account_keys_len = account_keys_len.saturating_add(1); // the executor program id

    if readonly_unsigned_count > usize::from(u8::MAX)
        || account_keys_len > usize::from(u8::MAX).saturating_add(1)
    {
        return Err(Error::TooManyAccountKeys(account_keys_len));
    }

    Ok(())
}

fn signature_is_present(signature: &impl AsRef<[u8]>) -> bool {
    signature.as_ref().iter().any(|byte| *byte != 0)
}
