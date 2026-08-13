use {
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_message::{
        MessageHeader, VersionedMessage,
        compiled_instruction::CompiledInstruction,
        legacy::Message,
        v0,
        v1::{self, TransactionConfig},
    },
    solana_signer::Signer as _,
    solana_system_interface::instruction as system_instruction,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_client::message::wrapped_message,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    spl_message_executor_client::instruction::execute,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::*,
    std::collections::BTreeSet,
};

#[test]
fn transaction_serialization_round_trips() {
    let (transaction, _nonce, _nonce_account) = valid_transaction();

    let bytes = wincode::serialize(&transaction).unwrap();
    let decoded = wincode::deserialize_exact::<VersionedTransaction>(&bytes).unwrap();

    assert_eq!(decoded, transaction);
    assert_eq!(verify_static(&decoded), Ok(()));
}

#[test]
fn sign_and_merge_are_order_independent() {
    let first = Keypair::new();
    let second = Keypair::new();
    let transaction_plan = TransactionPlan::new(
        Vec::new(),
        vec![first.pubkey(), second.pubkey()],
        Vec::new(),
        test_address(1),
    )
    .unwrap();
    let unsigned = build_transaction(
        &transaction_plan,
        Hash::new_from_array([1; 32]),
        genesis_hash(),
    )
    .unwrap();

    let mut first_half = unsigned.clone();
    let mut second_half = unsigned.clone();
    sign_transaction(&mut first_half, &first).unwrap();
    sign_transaction(&mut second_half, &second).unwrap();
    assert!(!is_fully_signed(&first_half));

    merge_transactions(&mut first_half, &second_half).unwrap();
    assert!(is_fully_signed(&first_half));

    let mut reverse_first = unsigned.clone();
    let mut reverse_second = unsigned;
    sign_transaction(&mut reverse_first, &second).unwrap();
    sign_transaction(&mut reverse_second, &first).unwrap();
    merge_transactions(&mut reverse_first, &reverse_second).unwrap();
    assert!(is_fully_signed(&reverse_first));
}

#[test]
fn invalid_signature_slot_does_not_count_as_signed() {
    let signer = Keypair::new();
    let transaction_plan =
        TransactionPlan::cancellation(test_address(14), signer.pubkey()).unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([14; 32]),
        genesis_hash(),
    )
    .unwrap();
    transaction.signatures[0] = [1; 64].into();

    assert_eq!(signer_status(&transaction), vec![(signer.pubkey(), false)]);
    assert!(!is_fully_signed(&transaction));
}

#[test]
fn verify_rejects_genesis_hash_mismatch() {
    let authority = Keypair::new();
    let transaction_plan =
        TransactionPlan::cancellation(test_address(18), authority.pubkey()).unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([18; 32]),
        genesis_hash(),
    )
    .unwrap();
    transaction
        .message
        .set_recent_blockhash(Hash::new_from_array([20; 32]));

    assert_eq!(
        verify_genesis_hash(&transaction, &genesis_hash()),
        Err(Error::GenesisHashMismatch)
    );
}

#[test]
fn submit_rejects_unexpected_extra_signer() {
    let authority = Keypair::new();
    let fee_payer = Keypair::new();
    let unexpected = Keypair::new();
    let transaction_plan =
        TransactionPlan::cancellation(test_address(21), authority.pubkey()).unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([21; 32]),
        genesis_hash(),
    )
    .unwrap();
    sign_transaction(&mut transaction, &authority).unwrap();

    assert_eq!(
        submit_transaction(
            &transaction,
            &fee_payer,
            &[&unexpected],
            Hash::new_from_array([23; 32]),
        ),
        Err(Error::OuterSignerNotRequired(unexpected.pubkey()))
    );
}

#[test]
fn merge_rejects_invalid_incoming_signature() {
    let signer = Keypair::new();
    let transaction_plan =
        TransactionPlan::cancellation(test_address(16), signer.pubkey()).unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([16; 32]),
        genesis_hash(),
    )
    .unwrap();
    let mut poisoned = transaction.clone();
    poisoned.signatures[0] = [2; 64].into();

    assert_eq!(
        merge_transactions(&mut transaction, &poisoned),
        Err(Error::InvalidSignature(signer.pubkey()))
    );
}

#[test]
fn cancellation_plan_builds_empty_inner_message() {
    let cold_authority = Keypair::new().pubkey();
    let transaction_plan = TransactionPlan::cancellation(test_address(2), cold_authority).unwrap();

    let transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([3; 32]),
        genesis_hash(),
    )
    .unwrap();
    let summary = inspect(&transaction).unwrap();

    assert!(summary.inner_instructions.is_empty());
    assert_eq!(
        summary.inner_required_signers[0],
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &cold_authority,)
    );
}

#[test]
fn second_programmatic_signer_meta_is_required_readonly_signer() {
    let first_authority = Keypair::new().pubkey();
    let second_authority = Keypair::new().pubkey();
    let second_programmatic_signer =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &second_authority);
    let instruction = Instruction {
        program_id: test_address(6),
        accounts: vec![AccountMeta::new_readonly(second_programmatic_signer, true)],
        data: Vec::new(),
    };
    let transaction_plan = TransactionPlan::new(
        vec![instruction],
        vec![first_authority, second_authority],
        Vec::new(),
        test_address(7),
    )
    .unwrap();

    let transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([4; 32]),
        genesis_hash(),
    )
    .unwrap();
    let summary = inspect(&transaction).unwrap();
    let inner_message = summary.inner_message.clone();

    assert_eq!(inner_message.header().num_required_signatures, 2);
    assert_eq!(inner_message.header().num_readonly_signed_accounts, 1);
    assert_eq!(
        summary.inner_required_signers[0],
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &first_authority,)
    );
    assert_eq!(
        summary.inner_required_signers[1],
        second_programmatic_signer
    );
    assert!(inner_message.is_signer(1));
    assert!(!inner_message.is_maybe_writable_with_reserved_addresses(1, None::<&BTreeSet<_>>));
}

#[test]
fn sign_populates_duplicate_required_signer_slots() {
    let signer = Keypair::new();
    let other = Keypair::new();
    let transaction_plan = TransactionPlan::new(
        Vec::new(),
        vec![signer.pubkey(), other.pubkey()],
        Vec::new(),
        test_address(8),
    )
    .unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        Hash::new_from_array([5; 32]),
        genesis_hash(),
    )
    .unwrap();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    message.account_keys[1] = signer.pubkey();

    sign_transaction(&mut transaction, &signer).unwrap();

    assert!(signature_is_present(&transaction.signatures[0]));
    assert!(signature_is_present(&transaction.signatures[1]));
    assert_eq!(transaction.signatures[0], transaction.signatures[1]);
}

#[test]
fn sign_only_output_builds_transaction_from_dumped_message() {
    let authority = Keypair::new().pubkey();
    let nonce_account = test_address(18);
    let nonce_value = Hash::new_from_array([19; 32]);
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
    let recipient = test_address(21);
    let mut inner_message = Message::new(
        &[system_instruction::transfer(
            &nonce_authority,
            &recipient,
            5,
        )],
        Some(&nonce_authority),
    );
    inner_message.recent_blockhash = nonce_value;
    let inner_message = VersionedMessage::Legacy(inner_message);
    let sign_only_json = serde_json::json!({
            "blockhash": nonce_value.to_string(),
            "message": STANDARD.encode(inner_message.serialize()),
            "absent": [nonce_authority.to_string()],
            "signers": ["11111111111111111111111111111111=1111111111111111111111111111111111111111111111111111111111111111"]
        })
        .to_string();
    let sign_only = SignOnlyTransaction::from_json(&sign_only_json).unwrap();

    let nonce = Nonce {
        nonce: nonce_value,
        authority: nonce_authority,
    };
    let transaction = transaction_from_sign_only_checked(
        &sign_only,
        nonce_account,
        &nonce,
        &[authority],
        &[],
        genesis_hash(),
    )
    .unwrap();

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Ok(())
    );

    let summary = inspect(&transaction).unwrap();
    assert_eq!(summary.genesis_hash, genesis_hash());
    assert_eq!(*summary.inner_message.recent_blockhash(), nonce_value);
    assert_eq!(summary.nonce_account, nonce_account);
    assert_eq!(summary.inner_required_signers, vec![nonce_authority]);
    assert_eq!(summary.inner_instructions.len(), 1);
    assert_eq!(
        summary.wrapper_signers,
        vec![SignerStatus {
            address: authority,
            signed: false,
        }]
    );
}

#[test]
fn sign_only_output_requires_dumped_message() {
    let sign_only = SignOnlyTransaction::from_json(
        &serde_json::json!({
            "blockhash": Hash::new_from_array([22; 32]).to_string(),
            "absent": []
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(sign_only.message(), Err(Error::MissingTransactionMessage));
}

#[test]
fn sign_only_output_rejects_blockhash_mismatch() {
    let message = Message {
        recent_blockhash: Hash::new_from_array([23; 32]),
        ..Message::default()
    };
    let sign_only = SignOnlyTransaction::from_json(
        &serde_json::json!({
            "blockhash": Hash::new_from_array([24; 32]).to_string(),
            "message": STANDARD.encode(VersionedMessage::Legacy(message).serialize())
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(sign_only.message(), Err(Error::SignOnlyLifetimeMismatch));
}

#[test]
fn sign_only_output_rejects_bad_signatures() {
    let sign_only = SignOnlyTransaction::from_json(
        &serde_json::json!({
            "blockhash": Hash::new_from_array([25; 32]).to_string(),
            "message": "unused",
            "badSig": [test_address(26).to_string()]
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(sign_only.message(), Err(Error::BadSignOnlySignatures));
}

#[test]
fn sign_only_output_rejects_invalid_base64_message() {
    let sign_only = SignOnlyTransaction::from_json(
        &serde_json::json!({
            "blockhash": Hash::new_from_array([38; 32]).to_string(),
            "message": "not base64!"
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(sign_only.message(), Err(Error::InvalidBase64));
}

#[test]
fn sign_only_output_rejects_non_message_payload() {
    let sign_only = SignOnlyTransaction::from_json(
        &serde_json::json!({
            "blockhash": Hash::new_from_array([39; 32]).to_string(),
            "message": STANDARD.encode([1, 2, 3])
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(sign_only.message(), Err(Error::InvalidInnerMessage));
}

#[test]
fn sign_only_output_accepts_required_submit_signer() {
    let authority = Keypair::new();
    let submit_signer = Keypair::new();
    let nonce_account = test_address(40);
    let nonce_value = Hash::new_from_array([41; 32]);
    let nonce_authority = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_interface::id(),
        &authority.pubkey(),
    );
    let instruction = Instruction {
        program_id: test_address(43),
        accounts: vec![
            AccountMeta::new(nonce_authority, true),
            AccountMeta::new_readonly(submit_signer.pubkey(), true),
        ],
        data: vec![1],
    };
    let mut inner_message = Message::new(&[instruction], Some(&nonce_authority));
    inner_message.recent_blockhash = nonce_value;
    let inner_message = VersionedMessage::Legacy(inner_message);
    let sign_only_json = serde_json::json!({
        "blockhash": nonce_value.to_string(),
        "message": STANDARD.encode(inner_message.serialize()),
        "absent": [nonce_authority.to_string(), submit_signer.pubkey().to_string()]
    })
    .to_string();
    let sign_only = SignOnlyTransaction::from_json(&sign_only_json).unwrap();
    let nonce = Nonce {
        nonce: nonce_value,
        authority: nonce_authority,
    };

    let mut transaction = transaction_from_sign_only_checked(
        &sign_only,
        nonce_account,
        &nonce,
        &[authority.pubkey()],
        &[submit_signer.pubkey()],
        genesis_hash(),
    )
    .unwrap();

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Ok(())
    );
    let summary = inspect(&transaction).unwrap();
    assert_eq!(
        summary.wrapper_signers,
        vec![
            SignerStatus {
                address: authority.pubkey(),
                signed: false,
            },
            SignerStatus {
                address: submit_signer.pubkey(),
                signed: false,
            },
        ]
    );

    sign_transaction(&mut transaction, &authority).unwrap();
    sign_transaction(&mut transaction, &submit_signer).unwrap();
    assert!(is_fully_signed(&transaction));
}

#[test]
fn from_message_rejects_too_many_wrapper_signers() {
    let first_authority = Keypair::new().pubkey();
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &first_authority);
    let mut authorities = vec![first_authority];
    while authorities.len() <= usize::from(u8::MAX) {
        authorities.push(Address::new_unique());
    }
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![nonce_authority],
        Hash::new_from_array([44; 32]),
        Vec::new(),
    ));

    assert_eq!(
        transaction_from_message(
            inner_message,
            test_address(45),
            &authorities,
            &[],
            genesis_hash(),
        ),
        Err(Error::TooManySigners(authorities.len()))
    );
}

#[test]
fn from_message_rejects_uncovered_inner_signer() {
    let authority = Keypair::new().pubkey();
    let inner_signer = test_address(25);
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![inner_signer],
        Hash::new_from_array([26; 32]),
        Vec::new(),
    ));

    assert_eq!(
        transaction_from_message(
            inner_message,
            test_address(27),
            &[authority],
            &[],
            genesis_hash(),
        ),
        Err(Error::RequiredInnerSignerNotCovered(inner_signer))
    );
}

#[test]
fn from_message_rejects_submit_signer_not_required_by_inner_message() {
    let authority = Keypair::new().pubkey();
    let submit_signer = test_address(27);
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![nonce_authority],
        Hash::new_from_array([28; 32]),
        Vec::new(),
    ));

    assert_eq!(
        transaction_from_message(
            inner_message,
            test_address(29),
            &[authority],
            &[submit_signer],
            genesis_hash(),
        ),
        Err(Error::SubmitSignerNotRequired(submit_signer))
    );
}

#[test]
fn from_message_rejects_programmatic_submit_signer() {
    let authority = Keypair::new().pubkey();
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![nonce_authority],
        Hash::new_from_array([31; 32]),
        Vec::new(),
    ));

    assert_eq!(
        transaction_from_message(
            inner_message,
            test_address(32),
            &[authority],
            &[nonce_authority],
            genesis_hash(),
        ),
        Err(Error::SubmitSignerCannotBeProgrammaticSigner(
            nonce_authority
        ))
    );
}

#[test]
fn from_message_checked_rejects_wrong_nonce_snapshot() {
    let authority = Keypair::new().pubkey();
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![nonce_authority],
        Hash::new_from_array([34; 32]),
        Vec::new(),
    ));

    assert_eq!(
        transaction_from_message_checked(
            inner_message,
            test_address(35),
            &Nonce {
                nonce: Hash::new_from_array([36; 32]),
                authority: nonce_authority,
            },
            &[authority],
            &[],
            genesis_hash(),
        ),
        Err(Error::NonceMismatch)
    );
}

#[test]
fn verify_rejects_wrong_nonce_value() {
    let (transaction, mut nonce, nonce_account) = valid_transaction();
    nonce.nonce = Hash::new_from_array([9; 32]);

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::NonceMismatch)
    );
}

#[test]
fn verify_rejects_wrong_genesis_hash() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    transaction
        .message
        .set_recent_blockhash(Hash::new_from_array([10; 32]));

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::GenesisHashMismatch)
    );
}

#[test]
fn verify_rejects_missing_authority_signer() {
    let (transaction, mut nonce, nonce_account) = valid_transaction();
    nonce.authority = test_address(3);

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::MissingNonceAuthority)
    );
}

#[test]
fn verify_rejects_two_wrapped_instructions() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    let instruction = message.instructions[0].clone();
    message.instructions.push(instruction);

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::InvalidExecutorInstructionCount)
    );
}

#[test]
fn verify_rejects_wrong_slot_hashes_account() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    let slot_hashes_index = usize::from(message.instructions[0].accounts[2]);
    message.account_keys[slot_hashes_index] = test_address(11);

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::InvalidWrappedTransaction)
    );
}

#[test]
fn verify_rejects_inner_account_order_mismatch() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    message.instructions[0].accounts.swap(3, 4);

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::InvalidWrappedTransaction)
    );
}

#[test]
fn verify_rejects_tampered_executor_program_id() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    message.instructions[0].program_id_index = message
        .account_keys
        .iter()
        .position(|key| *key == spl_nonce_interface::id())
        .and_then(|index| u8::try_from(index).ok())
        .unwrap();

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::InvalidExecutorProgramId)
    );
}

#[test]
fn verify_accepts_v0_inner_message_without_lookups() {
    let (transaction, nonce, nonce_account) =
        valid_versioned_transaction(|header, account_keys, recent_blockhash, instructions| {
            VersionedMessage::V0(v0::Message {
                header,
                account_keys,
                recent_blockhash,
                instructions,
                address_table_lookups: Vec::new(),
            })
        });

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Ok(())
    );
}

#[test]
fn verify_accepts_v1_inner_message_with_empty_config() {
    let (transaction, nonce, nonce_account) =
        valid_versioned_transaction(|header, account_keys, recent_blockhash, instructions| {
            VersionedMessage::V1(v1::Message::new(
                header,
                TransactionConfig::empty(),
                recent_blockhash,
                account_keys,
                instructions,
            ))
        });

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Ok(())
    );
}

#[test]
fn verify_rejects_uncovered_inner_signer() {
    let authority = Keypair::new().pubkey();
    let inner_signer = test_address(14);
    let nonce_account = test_address(15);
    let nonce_value = Hash::new_from_array([16; 32]);
    let inner_message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1,
        0,
        0,
        vec![inner_signer],
        nonce_value,
        Vec::new(),
    ));
    let transaction = transaction_for_inner_message(inner_message, authority, nonce_account);

    assert_eq!(
        verify(
            &transaction,
            &Nonce {
                nonce: nonce_value,
                authority: inner_signer,
            },
            &nonce_account,
            &genesis_hash(),
        ),
        Err(Error::MissingSignerPrivilege(inner_signer))
    );
}

#[test]
fn verify_rejects_missing_wrapped_writable_privilege() {
    let (mut transaction, nonce, nonce_account) = valid_transaction();
    let VersionedMessage::Legacy(message) = &mut transaction.message else {
        panic!("expected legacy message");
    };
    message.header.num_readonly_unsigned_accounts = u8::try_from(
        message
            .account_keys
            .len()
            .saturating_sub(usize::from(message.header.num_required_signatures)),
    )
    .unwrap();

    assert_eq!(
        verify(&transaction, &nonce, &nonce_account, &genesis_hash()),
        Err(Error::MissingWritablePrivilege(nonce.authority))
    );
}

fn valid_transaction() -> (VersionedTransaction, Nonce, Address) {
    let authority = Keypair::new();
    let nonce_account = test_address(4);
    let nonce_value = Hash::new_from_array([7; 32]);
    let transaction_plan =
        TransactionPlan::transfer(nonce_account, authority.pubkey(), test_address(5), 5).unwrap();
    let transaction = build_transaction(&transaction_plan, nonce_value, genesis_hash()).unwrap();
    let nonce_authority = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_interface::id(),
        &authority.pubkey(),
    );
    (
        transaction,
        Nonce {
            nonce: nonce_value,
            authority: nonce_authority,
        },
        nonce_account,
    )
}

fn valid_versioned_transaction(
    build_message: impl FnOnce(
        MessageHeader,
        Vec<Address>,
        Hash,
        Vec<CompiledInstruction>,
    ) -> VersionedMessage,
) -> (VersionedTransaction, Nonce, Address) {
    let authority = Keypair::new().pubkey();
    let nonce_account = test_address(12);
    let nonce_value = Hash::new_from_array([12; 32]);
    let nonce_authority =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
    let inner_message = build_message(
        MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        vec![nonce_authority],
        nonce_value,
        Vec::new(),
    );
    let transaction = transaction_for_inner_message(inner_message, authority, nonce_account);

    (
        transaction,
        Nonce {
            nonce: nonce_value,
            authority: nonce_authority,
        },
        nonce_account,
    )
}

fn transaction_for_inner_message(
    inner_message: VersionedMessage,
    authority: Address,
    nonce_account: Address,
) -> VersionedTransaction {
    let executor_instruction = execute(&nonce_account, &inner_message);
    let mut message = wrapped_message(&executor_instruction, &[authority]);
    message.set_recent_blockhash(genesis_hash());
    let required_signatures = usize::from(message.header().num_required_signatures);
    VersionedTransaction {
        signatures: core::iter::repeat_with(Default::default)
            .take(required_signatures)
            .collect(),
        message,
    }
}

fn signature_is_present(signature: &impl AsRef<[u8]>) -> bool {
    signature.as_ref().iter().any(|byte| *byte != 0)
}

fn test_address(byte: u8) -> Address {
    Address::new_from_array([byte; 32])
}

fn genesis_hash() -> Hash {
    Hash::new_from_array([250; 32])
}
