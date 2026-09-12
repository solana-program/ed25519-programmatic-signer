//! Exercise report editing through the extension trait, independently of wallets or Execute.

use {
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_message::{MessageHeader, VersionedMessage, legacy::Message, v0, v1},
    solana_signature::Signature,
    solana_signer::Signer,
    spl_programmatic_signer_cli::CliSignOnlyDataExt,
    std::{collections::BTreeMap, fs},
};

fn message_versions(authorities: &[Address]) -> [VersionedMessage; 3] {
    let legacy = Message {
        header: MessageHeader {
            num_required_signatures: u8::try_from(authorities.len()).unwrap(),
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        account_keys: authorities.to_vec(),
        recent_blockhash: Hash::new_from_array([9; 32]),
        instructions: vec![],
    };
    let v0 = v0::Message {
        header: legacy.header,
        account_keys: legacy.account_keys.clone(),
        recent_blockhash: legacy.recent_blockhash,
        instructions: vec![],
        address_table_lookups: vec![v0::MessageAddressTableLookup {
            account_key: Address::new_unique(),
            writable_indexes: vec![1],
            readonly_indexes: vec![2],
        }],
    };
    let v1 = v1::Message::new(
        legacy.header,
        v1::TransactionConfig::empty()
            .with_priority_fee(123)
            .with_compute_unit_limit(123_456)
            .with_loaded_accounts_data_size_limit(32_768)
            .with_heap_size(65_536),
        legacy.recent_blockhash,
        legacy.account_keys.clone(),
        vec![],
    );
    [
        VersionedMessage::Legacy(legacy),
        VersionedMessage::V0(v0),
        VersionedMessage::V1(v1),
    ]
}

fn report(message: &VersionedMessage) -> CliSignOnlyData {
    CliSignOnlyData {
        // Deliberately not the embedded blockhash: report editing must preserve this metadata.
        blockhash: "opaque producer metadata".to_string(),
        message: Some(BASE64_STANDARD.encode(message.serialize())),
        ..CliSignOnlyData::default()
    }
}

#[test]
fn adds_and_replaces_signatures_without_changing_message_or_blockhash() {
    let first = Keypair::new();
    let second = Keypair::new();
    let third = Keypair::new();
    let authorities = [first.pubkey(), second.pubkey(), third.pubkey()];
    for message in message_versions(&authorities) {
        let mut data = report(&message);
        let original_message = data.message.clone();
        let original_blockhash = data.blockhash.clone();
        let deserialized_message = data.deserialize_message().unwrap();
        assert_eq!(deserialized_message, message);
        let bytes = deserialized_message.serialize();
        assert_eq!(
            BASE64_STANDARD.encode(&bytes),
            original_message.as_ref().unwrap().as_str()
        );
        let first_sig = first.sign_message(&bytes);
        let second_sig = second.sign_message(&bytes);
        let first_entry = format!("{}={first_sig}", first.pubkey());
        let second_entry = format!("{}={second_sig}", second.pubkey());
        data.signers = vec![second_entry.clone(), second_entry.clone()];
        data.absent = vec![
            second.pubkey().to_string(),
            Address::new_unique().to_string(),
        ];
        data.bad_sig = vec![first.pubkey().to_string()];

        assert_eq!(
            data.verified_signatures().unwrap(),
            BTreeMap::from([(second.pubkey(), second_sig)])
        );
        data.add_signature(first.pubkey(), first_sig).unwrap();
        assert_eq!(data.signers, vec![first_entry, second_entry]);
        assert_eq!(data.absent, vec![third.pubkey().to_string()]);
        assert!(data.bad_sig.is_empty());
        let signed = serde_json::to_value(&data).unwrap();
        data.add_signature(first.pubkey(), first_sig).unwrap();
        assert_eq!(serde_json::to_value(&data).unwrap(), signed);

        assert_eq!(data.message, original_message);
        assert_eq!(data.blockhash, original_blockhash);
        assert_eq!(data.deserialize_message().unwrap(), message);
        assert_eq!(
            data.verified_signatures().unwrap(),
            BTreeMap::from([(first.pubkey(), first_sig), (second.pubkey(), second_sig)])
        );
    }
}

#[test]
fn invalid_additions_leave_the_entire_report_unchanged() {
    let authority = Keypair::new();
    let other = Keypair::new();
    for message in message_versions(&[authority.pubkey()]) {
        let mut data = report(&message);
        let bytes = data.deserialize_message().unwrap().serialize();
        data.add_signature(authority.pubkey(), authority.sign_message(&bytes))
            .unwrap();
        let before = serde_json::to_value(&data).unwrap();
        for (address, signature) in [
            (other.pubkey(), other.sign_message(&bytes)),
            (authority.pubkey(), Signature::default()),
            (authority.pubkey(), other.sign_message(&bytes)),
            (
                authority.pubkey(),
                authority.sign_message(b"different message"),
            ),
        ] {
            assert!(data.add_signature(address, signature).is_err());
            assert_eq!(serde_json::to_value(&data).unwrap(), before);
        }
    }
}

#[test]
fn invalid_existing_approvals_cannot_be_hidden_by_an_update() {
    let authority = Keypair::new();
    let other = Keypair::new();
    for message in message_versions(&[authority.pubkey()]) {
        let mut data = report(&message);
        let bytes = data.deserialize_message().unwrap().serialize();
        let key = authority.pubkey();
        let signature = authority.sign_message(&bytes);
        let valid_entry = format!("{key}={signature}");
        for entries in [
            vec![key.to_string()],
            vec![format!("invalid-address={signature}")],
            vec![format!("{key}=invalid-signature")],
            vec![format!("{key}={signature}=extra")],
            vec![format!("{key}={}", Signature::default())],
            vec![format!(
                "{key}={}",
                authority.sign_message(b"another message")
            )],
            vec![format!("{}={}", other.pubkey(), other.sign_message(&bytes))],
            vec![
                format!("{key}={}", Signature::default()),
                valid_entry.clone(),
            ],
            vec![
                valid_entry.clone(),
                format!("{key}={}", Signature::default()),
            ],
        ] {
            data.signers = entries;
            let before = serde_json::to_value(&data).unwrap();
            assert!(data.verified_signatures().is_err());
            assert!(data.add_signature(key, signature).is_err());
            assert_eq!(serde_json::to_value(&data).unwrap(), before);
        }
    }
}

#[test]
fn malformed_messages_are_rejected_without_modifying_the_report() {
    let authority = Keypair::new();
    let key = authority.pubkey();
    let signature = authority.sign_message(b"anything");
    let mut data = report(&message_versions(&[key])[0]);
    for encoded in [
        None,
        Some("!".to_string()),
        Some(BASE64_STANDARD.encode([0])),
    ] {
        data.message = encoded;
        let before = serde_json::to_value(&data).unwrap();
        assert!(data.deserialize_message().is_err());
        assert!(data.verified_signatures().is_err());
        assert!(data.add_signature(key, signature).is_err());
        assert_eq!(serde_json::to_value(&data).unwrap(), before);
    }

    for message in message_versions(&[key]) {
        let mut bytes = message.serialize();
        bytes.push(0);
        data.message = Some(BASE64_STANDARD.encode(&bytes));
        assert!(data.deserialize_message().is_err());
        let before = serde_json::to_value(&data).unwrap();
        assert!(data.verified_signatures().is_err());
        assert!(data.add_signature(key, signature).is_err());
        assert_eq!(serde_json::to_value(&data).unwrap(), before);
    }
}

#[test]
fn file_round_trip_preserves_the_report_and_never_overwrites() {
    let authority = Keypair::new();
    let directory = tempfile::tempdir().unwrap();
    for (index, message) in message_versions(&[authority.pubkey()])
        .into_iter()
        .enumerate()
    {
        let mut data = report(&message);
        data.add_signature(
            authority.pubkey(),
            authority.sign_message(&message.serialize()),
        )
        .unwrap();
        // File I/O does not normalize status metadata either.
        data.bad_sig.push("untrusted summary".to_string());
        let path = directory.path().join(format!("{index}.json"));
        data.write_new(&path).unwrap();
        assert_eq!(CliSignOnlyData::read(&path).unwrap(), data);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert!(data.write_new(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(data.write_new(directory.path()).is_err());
    }
}

#[test]
fn read_reports_missing_files_and_invalid_json_as_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.json");
    let error = CliSignOnlyData::read(&path).unwrap_err();
    assert!(error.to_string().contains("failed to read"));
    assert!(error.to_string().contains(path.to_str().unwrap()));
    fs::write(&path, "not JSON").unwrap();
    assert!(CliSignOnlyData::read(&path).is_err());
}

#[cfg(unix)]
#[test]
fn write_new_does_not_follow_live_or_dangling_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let authority = Keypair::new();
    let data = report(&message_versions(&[authority.pubkey()])[0]);
    for exists in [false, true] {
        let destination = directory.path().join(format!("destination-{exists}"));
        if exists {
            fs::write(&destination, "keep this file").unwrap();
        }
        let link = directory.path().join(format!("link-{exists}"));
        std::os::unix::fs::symlink(&destination, &link).unwrap();
        assert!(data.write_new(&link).is_err());
        assert!(link.symlink_metadata().unwrap().is_symlink());
        if exists {
            assert_eq!(fs::read_to_string(&destination).unwrap(), "keep this file");
        } else {
            assert!(!destination.exists());
        }
    }
}
