use {
    super::*,
    crate::artifact,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde_json::json,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_system_interface::instruction::transfer,
    std::fs,
    tempfile::TempDir,
};

fn fixture() -> (WrappedTransaction, Keypair, Nonce) {
    let cold = Keypair::new();
    let state = Nonce {
        authority: programmatic_signer(&cold.pubkey()),
        nonce: Hash::new_from_array([1; 32]),
    };
    let inner = Message::new_with_blockhash(
        &[transfer(
            &state.authority,
            &Address::new_unique(),
            1_000_000,
        )],
        Some(&state.authority),
        &state.nonce,
    );
    let transaction = WrappedTransaction::new(
        inner,
        Address::new_unique(),
        &[cold.pubkey()],
        &[],
        Hash::new_from_array([2; 32]),
    )
    .unwrap();
    (transaction, cold, state)
}

#[test]
fn signing_round_trip_and_nonce_verification() {
    let (mut transaction, cold, state) = fixture();
    let genesis = *transaction.genesis_hash();
    transaction.verify(&state, &genesis).unwrap();
    assert!(!transaction.is_fully_signed());
    let next = transaction.next_nonce();
    transaction.sign(&cold).unwrap();
    let decoded = WrappedTransaction::from_bytes(&transaction.to_bytes().unwrap()).unwrap();
    assert!(decoded.is_fully_signed());
    assert_eq!(decoded.next_nonce(), next);
    assert_eq!(decoded.inner(), transaction.inner());
    assert_eq!(decoded.nonce_account(), transaction.nonce_account());
    assert!(decoded.verify(&state, &Hash::default()).is_err());
    assert!(
        decoded
            .verify(
                &Nonce {
                    nonce: next,
                    ..state
                },
                &genesis
            )
            .is_err()
    );
    assert!(
        decoded
            .verify(
                &Nonce {
                    authority: Address::new_unique(),
                    ..state
                },
                &genesis
            )
            .is_err()
    );
}

#[test]
fn partial_signatures_merge_and_require_a_live_relayer() {
    let (first, cold, _) = fixture();
    let relayer = Keypair::new();
    let mut inner = first.inner().clone();
    // A Memo account makes the designated relayer an actual required inner signer.
    let instruction = Instruction {
        program_id: "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"
            .parse()
            .unwrap(),
        accounts: vec![AccountMeta::new_readonly(relayer.pubkey(), true)],
        data: b"approved".to_vec(),
    };
    inner = Message::new_with_blockhash(
        &[instruction],
        Some(&inner.account_keys[0]),
        &inner.recent_blockhash,
    );
    let mut first = WrappedTransaction::new(
        inner,
        *first.nonce_account(),
        &[cold.pubkey()],
        &[relayer.pubkey()],
        *first.genesis_hash(),
    )
    .unwrap();
    let mut second = WrappedTransaction::from_bytes(&first.to_bytes().unwrap()).unwrap();
    let payer = Keypair::new();
    assert!(first.relay(&payer, &[&relayer], Hash::default()).is_err());
    first.sign(&cold).unwrap();
    second.sign(&relayer).unwrap();
    first.merge(&second).unwrap();
    assert!(first.is_fully_signed());
    assert!(first.sign(&Keypair::new()).is_err());
    assert!(first.relay(&payer, &[], Hash::default()).is_err());
    assert!(
        first
            .relay(&payer, &[&relayer, &cold], Hash::default())
            .is_err()
    );
    let relay = first.relay(&payer, &[&relayer], Hash::default()).unwrap();
    relay.verify_and_hash_message().unwrap();
    let original = first.to_bytes().unwrap();
    assert!(first.merge(&fixture().0).is_err());
    assert_eq!(first.to_bytes().unwrap(), original);
}

#[test]
fn rejects_invalid_or_malformed_files() {
    let (transaction, _, _) = fixture();
    let mut cases = Vec::new();
    let original = &transaction.transaction;
    let mut bad = original.clone();
    bad.signatures[0] = [42; 64].into();
    cases.push(bad);
    let mut bad = original.clone();
    bad.signatures.clear();
    cases.push(bad);
    for mutate in [
        |message: &mut Message| message.account_keys[1] = message.account_keys[0],
        |message: &mut Message| message.instructions.clear(),
        |message: &mut Message| message.instructions.push(message.instructions[0].clone()),
        |message: &mut Message| message.instructions[0].program_id_index = 0,
        |message: &mut Message| message.instructions[0].data = vec![255],
        |message: &mut Message| message.instructions[0].accounts.swap(0, 1),
        |message: &mut Message| {
            message.instructions[0].accounts.pop();
        },
        |message: &mut Message| message.instructions[0].accounts[0] = 255,
        |message: &mut Message| {
            message.header.num_readonly_unsigned_accounts =
                u8::try_from(message.account_keys.len().saturating_sub(1)).unwrap()
        },
        |message: &mut Message| message.account_keys[0] = Address::new_unique(),
    ] {
        let mut bad = original.clone();
        let VersionedMessage::Legacy(message) = &mut bad.message else {
            unreachable!()
        };
        mutate(message);
        cases.push(bad);
    }
    for (index, bad) in cases.iter().enumerate() {
        assert!(
            WrappedTransaction::from_bytes(&wincode::serialize(bad).unwrap()).is_err(),
            "case {index}"
        );
    }
    let mut trailing = transaction.to_bytes().unwrap();
    trailing.push(0);
    assert!(WrappedTransaction::from_bytes(&trailing).is_err());
    assert!(WrappedTransaction::from_bytes(&[]).is_err());
}

#[test]
fn rejects_changed_signed_message_and_duplicate_signers() {
    let (mut transaction, cold, _) = fixture();
    transaction.sign(&cold).unwrap();
    transaction
        .transaction
        .message
        .set_recent_blockhash(Hash::default());
    assert!(WrappedTransaction::from_bytes(&transaction.to_bytes().unwrap()).is_err());
    let (transaction, cold, _) = fixture();
    for authorities in [
        vec![],
        vec![cold.pubkey(), cold.pubkey()],
        std::iter::once(cold.pubkey())
            .chain((0..255).map(|_| Address::new_unique()))
            .collect(),
    ] {
        assert!(
            WrappedTransaction::new(
                transaction.inner().clone(),
                *transaction.nonce_account(),
                &authorities,
                &[],
                *transaction.genesis_hash()
            )
            .is_err()
        );
    }
    assert!(
        WrappedTransaction::new(
            transaction.inner().clone(),
            *transaction.nonce_account(),
            &[Address::new_unique()],
            &[],
            *transaction.genesis_hash()
        )
        .is_err()
    );
    for signer in [Address::new_unique(), programmatic_signer(&cold.pubkey())] {
        assert!(
            WrappedTransaction::new(
                transaction.inner().clone(),
                *transaction.nonce_account(),
                &[cold.pubkey()],
                &[signer],
                *transaction.genesis_hash()
            )
            .is_err()
        );
    }
}

#[test]
fn wrapper_signer_limit_preserves_legacy_encoding() {
    let (transaction, cold, _) = fixture();
    let mut authorities = vec![cold.pubkey()];
    authorities.extend((0..126).map(|_| Address::new_unique()));
    let boundary = WrappedTransaction::new(
        transaction.inner().clone(),
        *transaction.nonce_account(),
        &authorities,
        &[],
        *transaction.genesis_hash(),
    )
    .unwrap();
    let decoded = WrappedTransaction::from_bytes(&boundary.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded.signer_status().count(), 127);
    authorities.push(Address::new_unique());
    assert!(
        WrappedTransaction::new(
            transaction.inner().clone(),
            *transaction.nonce_account(),
            &authorities,
            &[],
            *transaction.genesis_hash()
        )
        .is_err()
    );
}

#[test]
fn cancellation_is_empty_and_relay_size_is_bounded() {
    let (transaction, cold, state) = fixture();
    let cancellation = WrappedTransaction::new(
        Message::new_with_blockhash(&[], Some(&state.authority), &state.nonce),
        *transaction.nonce_account(),
        &[cold.pubkey()],
        &[],
        *transaction.genesis_hash(),
    )
    .unwrap();
    assert!(cancellation.inner().instructions.is_empty());
    assert_ne!(cancellation.next_nonce(), transaction.next_nonce());
    let inner = Message::new_with_blockhash(
        &[Instruction {
            program_id: Address::new_unique(),
            accounts: vec![],
            data: vec![0; 1100],
        }],
        Some(&state.authority),
        &state.nonce,
    );
    let mut large = WrappedTransaction::new(
        inner,
        *transaction.nonce_account(),
        &[cold.pubkey()],
        &[],
        *transaction.genesis_hash(),
    )
    .unwrap();
    large.sign(&cold).unwrap();
    assert!(
        large
            .relay(&Keypair::new(), &[], Hash::default())
            .unwrap_err()
            .to_string()
            .contains("maximum is 1232")
    );
}

#[test]
fn imports_upstream_sign_only_messages() {
    for (name, signer_count) in [
        ("agave_solana_transfer_sign_only.json", 1),
        ("spl_token_transfer_sign_only.json", 2),
        ("spl_token_multisig_transfer_sign_only.json", 3),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let message = artifact::read_message(&path).unwrap();
        assert_eq!(message.header.num_required_signatures, signer_count);
        let signers = message
            .signer_keys()
            .into_iter()
            .copied()
            .collect::<Vec<_>>();
        let state = Nonce {
            authority: signers[0],
            nonce: message.recent_blockhash,
        };
        let transaction = WrappedTransaction::new(
            message,
            Address::new_unique(),
            &[Address::new_unique()],
            &signers,
            Hash::default(),
        )
        .unwrap();
        transaction.verify(&state, &Hash::default()).unwrap();
    }
}

#[test]
fn rejects_invalid_sign_only_input() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("source.json");
    let (transaction, _, _) = fixture();
    let source = json!({"blockhash":transaction.inner().recent_blockhash.to_string(),
        "message":STANDARD.encode(transaction.inner().serialize())});
    let mut cases = vec![
        json!({"blockhash":source["blockhash"]}),
        json!({"blockhash":source["blockhash"],"message":"invalid base64"}),
    ];
    let mut bad = source.clone();
    bad["blockhash"] = Hash::default().to_string().into();
    cases.push(bad);
    let mut bad = source.clone();
    bad["badSig"] = json!(["invalid"]);
    cases.push(bad);
    let mut bad = source;
    bad["message"] = STANDARD
        .encode(
            wincode::serialize(&VersionedMessage::V0(solana_message::v0::Message::default()))
                .unwrap(),
        )
        .into();
    cases.push(bad);
    for source in cases {
        fs::write(&path, source.to_string()).unwrap();
        assert!(artifact::read_message(&path).is_err());
    }
}
