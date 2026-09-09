use {
    crate::{Error, Result, TransactionPlan},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::{AccountKeys, VersionedMessage, legacy::Message},
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
};

pub(crate) fn build_inner_message(
    transaction_plan: &TransactionPlan,
    nonce_value: Hash,
) -> Result<VersionedMessage> {
    let authority = transaction_plan
        .authorities
        .first()
        .ok_or(Error::EmptyAuthorities)?;
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), authority);
    if transaction_plan.submit_signers.contains(&nonce_authority) {
        return Err(Error::DuplicateAddress(nonce_authority));
    }

    let message = compile_legacy_inner_message(
        &transaction_plan.instructions,
        nonce_authority,
        &transaction_plan.submit_signers,
        nonce_value,
    )?;
    Ok(VersionedMessage::Legacy(message))
}

fn compile_legacy_inner_message(
    instructions: &[Instruction],
    nonce_authority: Address,
    submit_signers: &[Address],
    nonce_value: Hash,
) -> Result<Message> {
    let mut writable_signers = vec![nonce_authority];
    let mut readonly_signers = Vec::new();
    for submit_signer in submit_signers {
        if instruction_writes_key(instructions, submit_signer) {
            writable_signers.push(*submit_signer);
        } else {
            readonly_signers.push(*submit_signer);
        }
    }
    for instruction in instructions {
        for meta in &instruction.accounts {
            if !meta.is_signer
                || writable_signers.contains(&meta.pubkey)
                || readonly_signers.contains(&meta.pubkey)
            {
                continue;
            }
            if instruction_writes_key(instructions, &meta.pubkey) {
                writable_signers.push(meta.pubkey);
            } else {
                readonly_signers.push(meta.pubkey);
            }
        }
    }

    let required_signatures = writable_signers
        .len()
        .saturating_add(readonly_signers.len());
    let required_signatures_u8 = u8::try_from(required_signatures)
        .map_err(|_| Error::TooManySigners(required_signatures))?;
    let readonly_signers_u8 = u8::try_from(readonly_signers.len())
        .map_err(|_| Error::TooManySigners(required_signatures))?;

    let mut signer_keys = Vec::with_capacity(required_signatures);
    signer_keys.extend_from_slice(&writable_signers);
    signer_keys.extend_from_slice(&readonly_signers);

    let mut writable_unsigned = Vec::new();
    let mut readonly_unsigned = Vec::new();
    for instruction in instructions {
        for meta in &instruction.accounts {
            if signer_keys.contains(&meta.pubkey) {
                continue;
            }
            if meta.is_writable {
                push_writable(&mut writable_unsigned, &mut readonly_unsigned, meta.pubkey);
            } else {
                push_readonly(&writable_unsigned, &mut readonly_unsigned, meta.pubkey);
            }
        }
        if !signer_keys.contains(&instruction.program_id) {
            push_readonly(
                &writable_unsigned,
                &mut readonly_unsigned,
                instruction.program_id,
            );
        }
    }

    let readonly_unsigned_u8 = u8::try_from(readonly_unsigned.len()).map_err(|_| {
        Error::TooManyAccountKeys(
            signer_keys
                .len()
                .saturating_add(writable_unsigned.len())
                .saturating_add(readonly_unsigned.len()),
        )
    })?;
    let mut account_keys = Vec::with_capacity(
        signer_keys
            .len()
            .saturating_add(writable_unsigned.len())
            .saturating_add(readonly_unsigned.len()),
    );
    account_keys.extend_from_slice(&signer_keys);
    account_keys.extend_from_slice(&writable_unsigned);
    account_keys.extend_from_slice(&readonly_unsigned);

    if account_keys.len() > usize::from(u8::MAX).saturating_add(1) {
        return Err(Error::TooManyAccountKeys(account_keys.len()));
    }

    let compiled_instructions = AccountKeys::new(&account_keys, None)
        .try_compile_instructions(instructions)
        .map_err(|_| Error::MessageCompilation)?;

    Ok(Message::new_with_compiled_instructions(
        required_signatures_u8,
        readonly_signers_u8,
        readonly_unsigned_u8,
        account_keys,
        nonce_value,
        compiled_instructions,
    ))
}

fn instruction_writes_key(instructions: &[Instruction], key: &Address) -> bool {
    instructions.iter().any(|instruction| {
        instruction
            .accounts
            .iter()
            .any(|meta| meta.pubkey == *key && meta.is_writable)
    })
}

fn push_writable(writable: &mut Vec<Address>, readonly: &mut Vec<Address>, key: Address) {
    if writable.contains(&key) {
        return;
    }
    readonly.retain(|candidate| candidate != &key);
    writable.push(key);
}

fn push_readonly(writable: &[Address], readonly: &mut Vec<Address>, key: Address) {
    if writable.contains(&key) || readonly.contains(&key) {
        return;
    }
    readonly.push(key);
}
