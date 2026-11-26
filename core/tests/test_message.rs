use {
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::v1::{self, MessageError},
    solana_system_interface::instruction::transfer,
    spl_programmatic_signer_core::{
        instruction::{AuthoritySignature, Instruction as CoreInstruction, consume_nonce, execute},
        message::{ExecutionMessageError, execution_message},
        pda::ProgrammaticSigner,
    },
    std::collections::BTreeSet,
    test_case::test_case,
};

fn unique_addresses(count: usize) -> Vec<Address> {
    (0..count).map(|_| Address::new_unique()).collect()
}

#[test]
fn builds_transfer_message_with_pda_at_index_zero_and_cold_key_readonly() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let recipient = Address::new_unique();
    let expected_nonce = Hash::new_from_array([7; 32]);

    let ix = transfer(&pda, &recipient, 1_000_000);
    let message = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        expected_nonce,
        &[ix],
        &[authority],
    )
    .unwrap();

    assert_eq!(message.lifetime_specifier, expected_nonce);
    assert_eq!(message.header.num_required_signatures, 2);
    assert_eq!(message.header.num_readonly_signed_accounts, 1);
    assert_eq!(message.account_keys[0], pda);
    assert_eq!(message.account_keys[1], authority);
    assert_eq!(message.instructions.len(), 2);

    let compiled_consume = &message.instructions[0];
    assert_eq!(
        message.account_keys[usize::from(compiled_consume.program_id_index)],
        program_id
    );
    assert_eq!(compiled_consume.accounts.len(), 2);
    assert_eq!(
        message.account_keys[usize::from(compiled_consume.accounts[0])],
        nonce_account
    );
    assert_eq!(
        message.account_keys[usize::from(compiled_consume.accounts[1])],
        pda
    );
    assert_eq!(
        compiled_consume.data,
        consume_nonce(&program_id, &nonce_account, &pda).data
    );
}

#[test]
fn supports_empty_payload_for_pda_nonce_cancellation() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let expected_nonce = Hash::new_from_array([9; 32]);

    let message = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        expected_nonce,
        &[],
        &[authority],
    )
    .unwrap();

    assert_eq!(message.instructions.len(), 1);
    assert_eq!(message.header.num_required_signatures, 2);
    assert_eq!(message.header.num_readonly_signed_accounts, 1);
    assert_eq!(message.account_keys[0], pda);
    assert_eq!(message.account_keys[1], authority);
}

#[test]
fn keeps_readonly_nonce_authority_when_another_signer_is_writable() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let eoa_nonce_authority = Address::new_unique();
    let recipient = Address::new_unique();

    let ix = transfer(&pda, &recipient, 1_000_000);
    let message = execution_message(
        &program_id,
        &nonce_account,
        &eoa_nonce_authority,
        Hash::new_from_array([4; 32]),
        &[ix],
        &[authority],
    )
    .unwrap();

    assert_eq!(message.header.num_required_signatures, 3);
    assert_eq!(message.header.num_readonly_signed_accounts, 2);
    assert_eq!(message.account_keys[0], pda);
    let nonce_auth_idx = message
        .account_keys
        .iter()
        .position(|key| *key == eoa_nonce_authority)
        .unwrap();
    assert!(message.is_signer(nonce_auth_idx));
    assert!(
        !message.is_maybe_writable_with_reserved_addresses(nonce_auth_idx, None::<&BTreeSet<_>>)
    );
}

#[test]
fn avoids_invoked_program_at_index_zero() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let target_program = Address::new_unique();

    // Invoked program marked writable signer alongside read-only `nonce_authority` upgrades
    // `nonce_authority` to take index 0
    let ix = Instruction {
        program_id: target_program,
        accounts: vec![AccountMeta::new(target_program, true)],
        data: vec![],
    };
    let upgraded = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        Hash::new_from_array([5; 32]),
        &[ix],
        &[authority],
    )
    .unwrap();
    assert_eq!(upgraded.account_keys[0], pda);
    assert_eq!(upgraded.account_keys[1], target_program);
    assert_eq!(upgraded.header.num_required_signatures, 3);
    assert_eq!(upgraded.header.num_readonly_signed_accounts, 1);

    // Invoked writable signer listed before another non-invoked writable signer still yields
    // index 0 to the non-invoked signer without making `nonce_authority` writable
    let other_signer = Address::new_unique();
    let ix = Instruction {
        program_id: target_program,
        accounts: vec![
            AccountMeta::new(target_program, true),
            AccountMeta::new(other_signer, true),
        ],
        data: vec![],
    };
    let reordered = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        Hash::new_from_array([6; 32]),
        &[ix],
        &[authority],
    )
    .unwrap();
    assert_eq!(reordered.account_keys[0], other_signer);
    assert_eq!(reordered.account_keys[1], target_program);
    assert_eq!(reordered.header.num_required_signatures, 4);
    assert_eq!(reordered.header.num_readonly_signed_accounts, 2);

    // Invoked `nonce_authority` falls back to upgrading the first non-invoked signer
    let fallback = execution_message(
        &program_id,
        &nonce_account,
        &program_id,
        Hash::new_from_array([7; 32]),
        &[],
        &[authority],
    )
    .unwrap();
    assert_eq!(fallback.account_keys[0], authority);
    assert_eq!(fallback.account_keys[1], program_id);
    assert_eq!(fallback.header.num_required_signatures, 2);
    assert_eq!(fallback.header.num_readonly_signed_accounts, 1);
}

#[test]
fn upgrades_unreferenced_authority_to_keep_payload_pda_readonly() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let token_program = Address::new_unique();
    let source_ata = Address::new_unique();
    let dest_ata = Address::new_unique();

    // Mirrors an SPL Token transfer where `pda` is a read-only signer and `authority` is not
    // referenced by the payload instruction
    let token_transfer = Instruction {
        program_id: token_program,
        accounts: vec![
            AccountMeta::new(source_ata, false),
            AccountMeta::new(dest_ata, false),
            AccountMeta::new_readonly(pda, true),
        ],
        data: vec![3],
    };
    let message = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        Hash::new_from_array([10; 32]),
        &[token_transfer],
        &[authority],
    )
    .unwrap();

    assert_eq!(message.header.num_required_signatures, 2);
    assert_eq!(message.header.num_readonly_signed_accounts, 1);
    assert_eq!(message.account_keys[0], authority);
    assert_eq!(message.account_keys[1], pda);
    assert!(!message.is_maybe_writable_with_reserved_addresses(1, None::<&BTreeSet<_>>));

    // Outer `Execute` instruction skips write lock on `authority` at index 0 since no payload
    // instruction uses it
    let outer_ix = execute::<u8>(
        &program_id,
        vec![AuthoritySignature {
            account_index: 0,
            signature: 1,
        }],
        vec![],
        &message,
    );
    let meta = |key: Address| outer_ix.accounts.iter().find(|m| m.pubkey == key).unwrap();
    assert!(!meta(authority).is_writable);
    assert!(!meta(pda).is_writable);
    assert!(meta(nonce_account).is_writable);
    assert!(meta(source_ata).is_writable);
    assert!(meta(dest_ata).is_writable);
}

#[test]
fn rejects_all_readonly_signers_when_every_signer_is_in_payload() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let target_program = Address::new_unique();
    let target_account = Address::new_unique();

    // EOA `nonce_authority` referenced read-only with no other unused signer
    let eoa = Address::new_unique();
    let eoa_ix = Instruction {
        program_id: target_program,
        accounts: vec![
            AccountMeta::new(target_account, false),
            AccountMeta::new_readonly(eoa, true),
        ],
        data: vec![],
    };
    assert_eq!(
        execution_message(
            &program_id,
            &nonce_account,
            &eoa,
            Hash::default(),
            &[eoa_ix],
            &[],
        ),
        Err(ExecutionMessageError::NoWritableSigner)
    );

    // Both `pda` and `authority` referenced read-only in the payload
    let dual_ix = Instruction {
        program_id: target_program,
        accounts: vec![
            AccountMeta::new(target_account, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new_readonly(pda, true),
        ],
        data: vec![],
    };
    assert_eq!(
        execution_message(
            &program_id,
            &nonce_account,
            &pda,
            Hash::default(),
            &[dual_ix],
            &[authority],
        ),
        Err(ExecutionMessageError::NoWritableSigner)
    );
}

#[test]
fn deduplicates_accounts_and_merges_privileges_across_instructions() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let target_program = Address::new_unique();
    let ro_then_rw = Address::new_unique();
    let rw_then_ro = Address::new_unique();
    let readonly = Address::new_unique();
    let signer_then_nonsigner = Address::new_unique();
    let nonsigner_then_signer = Address::new_unique();

    let ix_1 = Instruction {
        program_id: target_program,
        accounts: vec![
            AccountMeta::new_readonly(ro_then_rw, false),
            AccountMeta::new(rw_then_ro, false),
            AccountMeta::new_readonly(readonly, false),
            AccountMeta::new_readonly(authority, false),
            AccountMeta::new_readonly(signer_then_nonsigner, true),
            AccountMeta::new_readonly(nonsigner_then_signer, false),
        ],
        data: vec![1],
    };
    let ix_2 = Instruction {
        program_id: target_program,
        accounts: vec![
            AccountMeta::new(ro_then_rw, false),
            AccountMeta::new_readonly(rw_then_ro, false),
            AccountMeta::new(authority, false),
            AccountMeta::new_readonly(signer_then_nonsigner, false),
            AccountMeta::new_readonly(nonsigner_then_signer, true),
        ],
        data: vec![2],
    };

    let message = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        Hash::new_from_array([2; 32]),
        &[ix_1.clone(), ix_2.clone()],
        &[authority, authority],
    )
    .unwrap();

    let unique: BTreeSet<_> = message.account_keys.iter().copied().collect();
    assert_eq!(unique.len(), message.account_keys.len());

    // Both payload instructions recompile every account index and data byte accurately
    for (compiled, original) in message.instructions[1..].iter().zip([&ix_1, &ix_2]) {
        assert_eq!(
            message.account_keys[usize::from(compiled.program_id_index)],
            original.program_id
        );
        assert_eq!(compiled.data, original.data);
        let resolved: Vec<_> = compiled
            .accounts
            .iter()
            .map(|&idx| message.account_keys[usize::from(idx)])
            .collect();
        let expected: Vec<_> = original.accounts.iter().map(|m| m.pubkey).collect();
        assert_eq!(resolved, expected);
    }

    // `authority` is the writable signer at index 0, while the other 3 signers stay read-only
    assert_eq!(message.header.num_required_signatures, 4);
    assert_eq!(message.header.num_readonly_signed_accounts, 3);
    let index_of = |key: Address| message.account_keys.iter().position(|k| *k == key).unwrap();

    for key in [authority, pda, signer_then_nonsigner, nonsigner_then_signer] {
        assert!(message.is_signer(index_of(key)));
    }
    for key in [authority, nonce_account, ro_then_rw, rw_then_ro] {
        assert!(
            message.is_maybe_writable_with_reserved_addresses(index_of(key), None::<&BTreeSet<_>>)
        );
    }
    for key in [
        readonly,
        target_program,
        pda,
        signer_then_nonsigner,
        nonsigner_then_signer,
    ] {
        assert!(
            !message.is_maybe_writable_with_reserved_addresses(index_of(key), None::<&BTreeSet<_>>)
        );
    }
}

#[test]
fn accepts_exact_signature_and_address_limits() {
    let program_id = Address::new_unique();
    let nonce_account = Address::new_unique();
    let nonce_authority = Address::new_unique();
    let target_program = Address::new_unique();
    let authorities = unique_addresses(usize::from(v1::MAX_SIGNATURES).checked_sub(1).unwrap());
    // Subtract the 3 fixed unsigned accounts (`program_id`, `nonce_account`, `target_program`)
    let extra_accounts = usize::from(v1::MAX_ADDRESSES)
        .checked_sub(usize::from(v1::MAX_SIGNATURES))
        .unwrap()
        .checked_sub(3)
        .unwrap();
    let ix = Instruction {
        program_id: target_program,
        accounts: unique_addresses(extra_accounts)
            .into_iter()
            .map(|key| AccountMeta::new(key, false))
            .collect(),
        data: vec![],
    };

    let message = execution_message(
        &program_id,
        &nonce_account,
        &nonce_authority,
        Hash::default(),
        &[ix],
        &authorities,
    )
    .unwrap();

    assert_eq!(message.header.num_required_signatures, v1::MAX_SIGNATURES);
    assert_eq!(message.account_keys.len(), usize::from(v1::MAX_ADDRESSES));
}

#[test]
fn enforces_instruction_limit() {
    let program_id = Address::new_unique();
    let nonce_account = Address::new_unique();
    let nonce_authority = Address::new_unique();
    let target_program = Address::new_unique();
    let ix = Instruction::new_with_bytes(target_program, &[], vec![]);
    let max_payload = usize::from(v1::MAX_INSTRUCTIONS).checked_sub(1).unwrap();

    let message = execution_message(
        &program_id,
        &nonce_account,
        &nonce_authority,
        Hash::default(),
        &vec![ix.clone(); max_payload],
        &[],
    )
    .unwrap();
    assert_eq!(
        message.instructions.len(),
        usize::from(v1::MAX_INSTRUCTIONS)
    );

    assert_eq!(
        execution_message(
            &program_id,
            &nonce_account,
            &nonce_authority,
            Hash::default(),
            &vec![ix; usize::from(v1::MAX_INSTRUCTIONS)],
            &[],
        ),
        Err(ExecutionMessageError::Invalid(
            MessageError::TooManyInstructions
        )),
    );
}

#[test]
fn rejects_too_many_signatures() {
    let program_id = Address::new_unique();
    let nonce_account = Address::new_unique();
    let nonce_authority = Address::new_unique();
    let authorities = unique_addresses(usize::from(v1::MAX_SIGNATURES));

    assert_eq!(
        execution_message(
            &program_id,
            &nonce_account,
            &nonce_authority,
            Hash::default(),
            &[],
            &authorities,
        ),
        Err(ExecutionMessageError::Invalid(
            MessageError::TooManySignatures
        )),
    );
}

#[test_case(1; "one over the v1 limit")]
#[test_case(300; "beyond u8 indexes")]
fn rejects_too_many_account_keys(excess: usize) {
    let program_id = Address::new_unique();
    let nonce_account = Address::new_unique();
    let nonce_authority = Address::new_unique();
    let authority = Address::new_unique();
    let target_program = Address::new_unique();
    // Subtract the 5 fixed accounts (`program_id`, `nonce_account`, `nonce_authority`,
    // `authority`, and `target_program`)
    let count = usize::from(v1::MAX_ADDRESSES)
        .checked_sub(5)
        .unwrap()
        .saturating_add(excess);
    let ix = Instruction {
        program_id: target_program,
        accounts: unique_addresses(count)
            .into_iter()
            .map(|key| AccountMeta::new(key, false))
            .collect(),
        data: vec![],
    };

    assert_eq!(
        execution_message(
            &program_id,
            &nonce_account,
            &nonce_authority,
            Hash::default(),
            &[ix],
            &[authority],
        ),
        Err(ExecutionMessageError::Invalid(
            MessageError::TooManyAddresses
        )),
    );
}

#[test]
fn execute_builds_instruction_from_message_and_sets_relay_signers() {
    let program_id = Address::new_unique();
    let authority = Address::new_unique();
    let pda = ProgrammaticSigner::derive_address(&program_id, &authority);
    let nonce_account = Address::new_unique();
    let recipient = Address::new_unique();
    let expected_nonce = Hash::new_from_array([11; 32]);

    let ix = transfer(&pda, &recipient, 1_000_000);
    let message = execution_message(
        &program_id,
        &nonce_account,
        &pda,
        expected_nonce,
        &[ix],
        &[authority],
    )
    .unwrap();

    // `pda` is at index 0, `authority` is at index 1 and also listed as a relay signer
    let signatures = vec![AuthoritySignature {
        account_index: 1,
        signature: [42u8; 64],
    }];
    let relay_signers = vec![1];
    let outer_ix = execute(
        &program_id,
        signatures.clone(),
        relay_signers.clone(),
        &message,
    );

    assert_eq!(outer_ix.program_id, program_id);
    let account_pubkeys: Vec<_> = outer_ix.accounts.iter().map(|m| m.pubkey).collect();
    assert_eq!(account_pubkeys, message.account_keys);

    // Listed relay signers start with `is_signer = true`, while all other accounts start false
    let meta = |key: Address| outer_ix.accounts.iter().find(|m| m.pubkey == key).unwrap();
    assert!(!meta(pda).is_signer);
    assert!(meta(pda).is_writable);
    assert!(meta(authority).is_signer);
    assert!(!meta(authority).is_writable);
    assert!(!meta(nonce_account).is_signer);
    assert!(meta(nonce_account).is_writable);
    assert!(!meta(recipient).is_signer);
    assert!(meta(recipient).is_writable);
    assert!(!meta(program_id).is_signer);
    assert!(!meta(program_id).is_writable);

    // Wire data round-trips back to the exact signatures, relay signers, header, nonce, and instructions
    let decoded = CoreInstruction::<[u8; 64]>::try_from_bytes(&outer_ix.data).unwrap();
    assert_eq!(
        decoded,
        CoreInstruction::Execute {
            signatures,
            relay_signers,
            header: message.header,
            expected_nonce,
            instructions: message.instructions,
        }
    );
}
