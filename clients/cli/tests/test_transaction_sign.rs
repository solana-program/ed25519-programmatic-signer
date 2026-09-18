use {
    crate::common::helpers::{run_psigner, run_psigner_with_input},
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_clap_v3_utils::input_parsers::signer::PubkeySignature,
    solana_cli_config::Config as SolanaConfig,
    solana_cli_output::CliSignOnlyData,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::{VersionedMessage, legacy::Message, v0, v1},
    solana_signature::Signature,
    solana_signer::Signer,
    solana_system_interface::instruction::transfer,
    spl_ed25519_signer_client::{ProgrammaticSigner, message::wrapped_message},
    spl_legacy_message_executor_client::instruction::execute,
    spl_legacy_message_executor_interface::instruction::Instruction as ExecutorInstruction,
    std::{fs, path::PathBuf},
    tempfile::TempDir,
    test_case::test_case,
};

pub mod common;

/// Build an approval fixture containing one `Execute` instruction. The inner message transfers
/// one lamport from each authority's programmatic signer to a fixed recipient.
fn approval_message(authorities: &[Address]) -> Message {
    let transfers = authorities
        .iter()
        .map(|authority| {
            let signer =
                ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority);
            transfer(&signer, &Address::new_from_array([3; 32]), 1)
        })
        .collect::<Vec<_>>();
    let inner = Message::new_with_blockhash(&transfers, None, &Hash::new_from_array([8; 32]));
    let instruction = execute(
        &Address::new_from_array([2; 32]),
        &inner.account_keys[0],
        &inner,
    );
    let VersionedMessage::Legacy(outer) = wrapped_message(&instruction, authorities) else {
        panic!("wrapped_message must produce a legacy message");
    };
    outer
}

fn sign_only(msg: &VersionedMessage) -> CliSignOnlyData {
    CliSignOnlyData {
        blockhash: msg.recent_blockhash().to_string(),
        message: Some(BASE64_STANDARD.encode(msg.serialize())),
        ..CliSignOnlyData::default()
    }
}

struct SignTestEnv {
    directory: TempDir,
    config_file_path: String,
    authority: Keypair,
    input: PathBuf,
}

impl SignTestEnv {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        // Fixed keys keep approval-screen goldens reproducible
        let authority = Keypair::new_from_array([1; 32]);
        let keypair_file = directory.path().join("authority.json");
        write_keypair_file(&authority, &keypair_file).unwrap();
        let config_file_path = directory
            .path()
            .join("config.yml")
            .to_str()
            .unwrap()
            .to_string();
        SolanaConfig {
            // Signing must succeed without a working RPC endpoint
            json_rpc_url: "http://127.0.0.1:1".into(),
            keypair_path: keypair_file.to_str().unwrap().into(),
            ..SolanaConfig::default()
        }
        .save(&config_file_path)
        .unwrap();

        let env = Self {
            input: directory.path().join("unsigned.json"),
            directory,
            config_file_path,
            authority,
        };
        env.write_input(&sign_only(&VersionedMessage::Legacy(approval_message(&[
            env.authority.pubkey(),
        ]))));
        env
    }

    fn write_input(&self, data: &CliSignOnlyData) {
        fs::write(&self.input, serde_json::to_vec(data).unwrap()).unwrap();
    }

    fn args<'a>(&'a self, extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec![
            "-C",
            &self.config_file_path,
            "transaction",
            "sign",
            self.input.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        args
    }

    fn assert_rejected(&self, expected: &str) {
        let original = fs::read(&self.input).unwrap();
        let output = run_psigner_with_input(&self.args(&["--yes"]), "");
        let stderr = String::from_utf8(output.stderr).unwrap();

        assert!(!output.status.success(), "unexpected success: {stderr}");
        assert!(stderr.contains(expected), "{stderr}");
        assert!(output.stdout.is_empty());
        assert!(!stderr.contains("Sign this approval?"));
        assert_eq!(fs::read(&self.input).unwrap(), original);
    }
}

#[test_case(VersionedMessage::Legacy, "legacy"; "legacy")]
#[test_case(|message: Message| VersionedMessage::V0(v0::Message {
    header: message.header,
    account_keys: message.account_keys,
    recent_blockhash: message.recent_blockhash,
    instructions: message.instructions,
    address_table_lookups: vec![],
}), "v0"; "v0")]
#[test_case(|message: Message| VersionedMessage::V1(v1::Message::new(
    message.header,
    v1::TransactionConfig::empty(),
    message.recent_blockhash,
    message.account_keys,
    message.instructions,
)), "v1"; "v1")]
fn signs_outer_message_and_displays_approval_status(
    version: fn(Message) -> VersionedMessage,
    version_name: &str,
) {
    let env = SignTestEnv::new();
    let cosigner = Keypair::new_from_array([4; 32]);
    let message = version(approval_message(&[
        env.authority.pubkey(),
        cosigner.pubkey(),
    ]));
    let cosignature = cosigner.sign_message(&message.serialize());
    let mut data = sign_only(&message);
    data.signers = vec![format!("{}={cosignature}", cosigner.pubkey())];
    data.absent = vec![cosigner.pubkey().to_string()];
    data.bad_sig = vec![cosigner.pubkey().to_string()];
    data.blockhash = Hash::new_from_array([99; 32]).to_string();
    env.write_input(&data);
    let original = fs::read(&env.input).unwrap();

    let signature = env.authority.sign_message(&message.serialize());
    let file_count = fs::read_dir(env.directory.path()).unwrap().count();
    for format in ["display", "json", "json-compact"] {
        let output = run_psigner(&env.args(&["--yes", "--output", format]));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(&format!("{version_name} message:\n")));
        assert!(stderr.contains("Approval signatures: 1 of 2 present"));
        assert!(stderr.contains(&format!("{}: present (verified)", cosigner.pubkey())));
        assert!(stderr.contains(&format!(
            "{}: not included (your signing key)",
            env.authority.pubkey()
        )));
        assert!(!stderr.contains("Sign this approval?"));
        if format == "display" {
            let stdout = String::from_utf8(output.stdout).unwrap();
            stdout.trim().parse::<PubkeySignature>().unwrap();
            assert_eq!(stdout, format!("{}={signature}\n", env.authority.pubkey()));
        } else {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                serde_json::json!({
                    "address": env.authority.pubkey().to_string(),
                    "signature": signature.to_string(),
                })
            );
        }
        assert_eq!(fs::read(&env.input).unwrap(), original);
        assert_eq!(
            fs::read_dir(env.directory.path()).unwrap().count(),
            file_count
        );
    }
}

#[test]
fn approval_screen_matches_golden() {
    let env = SignTestEnv::new();
    let output = run_psigner(&env.args(&["--yes"]));
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(stderr, include_str!("goldens/transaction-approval.txt"));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}={}\n",
            env.authority.pubkey(),
            env.authority
                .sign_message(&approval_message(&[env.authority.pubkey()]).serialize())
        )
    );
}

#[test_case("display"; "display")]
#[test_case("json"; "json")]
#[test_case("json-compact"; "json compact")]
fn quiet_mode_preserves_signature_output(format: &str) {
    let env = SignTestEnv::new();
    let normal = run_psigner(&env.args(&["--yes", "--output", format]));
    let quiet = run_psigner(&env.args(&["--quiet", "--yes", "--output", format]));

    assert!(!normal.stderr.is_empty());
    assert!(quiet.stderr.is_empty());
    assert_eq!(quiet.stdout, normal.stdout);
}

#[test]
fn quiet_mode_preserves_validation_errors() {
    let env = SignTestEnv::new();
    let mut message = approval_message(&[env.authority.pubkey()]);
    message.recent_blockhash = Hash::new_from_array([9; 32]);
    env.write_input(&sign_only(&VersionedMessage::Legacy(message)));

    let output = run_psigner_with_input(&env.args(&["--quiet", "--yes"]), "");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        stderr.contains("outer message must use the default blockhash"),
        "{stderr}"
    );
    assert!(!stderr.contains("Sign this approval?"));
}

#[test_case("y\n", true; "accept")]
#[test_case("n\n", false; "decline")]
#[test_case("", false; "end of input")]
fn quiet_mode_preserves_confirmation(answer: &str, approved: bool) {
    let env = SignTestEnv::new();
    let output = run_psigner_with_input(&env.args(&["--quiet"]), answer);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(stderr.starts_with("Sign this approval? [y/N] "), "{stderr}");
    assert_eq!(output.status.success(), approved, "{stderr}");
    assert_eq!(output.stdout.is_empty(), !approved);
    if approved {
        assert_eq!(stderr, "Sign this approval? [y/N] ");
    } else {
        assert!(stderr.contains("signing cancelled"), "{stderr}");
    }
}

#[test]
fn reports_input_path_when_read_fails() {
    let env = SignTestEnv::new();
    let missing = env.directory.path().join("missing.json");
    let output = run_psigner_with_input(
        &[
            "-C",
            &env.config_file_path,
            "transaction",
            "sign",
            missing.to_str().unwrap(),
            "--yes",
        ],
        "",
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&format!("failed to read {}", missing.display()))
    );
}

#[test_case("not JSON"; "malformed json")]
#[test_case("{}"; "missing fields")]
fn rejects_invalid_json(contents: &str) {
    let env = SignTestEnv::new();
    fs::write(&env.input, contents).unwrap();

    env.assert_rejected("invalid sign-only JSON");
}

#[test_case(None, "missing transaction message"; "missing message")]
#[test_case(Some("!"), "invalid base64 message"; "invalid base64")]
#[test_case(Some(""), "invalid serialized message"; "empty message")]
#[test_case(Some("AA=="), "invalid serialized message"; "truncated message")]
fn rejects_invalid_message_encoding(encoded: Option<&str>, expected: &str) {
    let env = SignTestEnv::new();
    env.write_input(&CliSignOnlyData {
        message: encoded.map(str::to_owned),
        ..CliSignOnlyData::default()
    });

    env.assert_rejected(expected);
}

#[test]
fn rejects_trailing_message_bytes() {
    let env = SignTestEnv::new();
    let message = VersionedMessage::Legacy(approval_message(&[env.authority.pubkey()]));
    let mut bytes = message.serialize();
    bytes.push(0);
    let mut data = sign_only(&message);
    data.message = Some(BASE64_STANDARD.encode(bytes));
    env.write_input(&data);

    env.assert_rejected("invalid serialized message");
}

#[test_case(0, "missing approval authorities"; "no required signers")]
#[test_case(127, "missing approval authority addresses"; "not enough account keys")]
fn rejects_invalid_authority_counts(count: u8, expected: &str) {
    let env = SignTestEnv::new();
    let mut message = approval_message(&[env.authority.pubkey()]);
    message.header.num_required_signatures = count;
    env.write_input(&sign_only(&VersionedMessage::Legacy(message)));

    env.assert_rejected(expected);
}

#[test_case("missing separator", "invalid signer: expected ADDRESS=SIGNATURE"; "missing separator")]
#[test_case("invalid=invalid", "invalid signer address"; "invalid address")]
#[test_case("11111111111111111111111111111111=invalid", "invalid signer signature"; "invalid signature")]
fn rejects_malformed_supplied_signatures(entry: &str, expected: &str) {
    let env = SignTestEnv::new();
    let message = VersionedMessage::Legacy(approval_message(&[env.authority.pubkey()]));
    let mut data = sign_only(&message);
    data.signers.push(entry.into());
    env.write_input(&data);

    env.assert_rejected(expected);
}

#[test]
fn rejects_supplied_signatures_from_non_authorities() {
    let env = SignTestEnv::new();
    let outsider = Keypair::new_from_array([4; 32]);
    let mut message = approval_message(&[env.authority.pubkey()]);
    // Appearing as an unsigned account does not make this address an approval authority.
    message.account_keys.push(outsider.pubkey());
    message.header.num_readonly_unsigned_accounts = message
        .header
        .num_readonly_unsigned_accounts
        .checked_add(1)
        .unwrap();
    let message = VersionedMessage::Legacy(message);
    let mut data = sign_only(&message);
    data.signers.push(format!(
        "{}={}",
        outsider.pubkey(),
        outsider.sign_message(&message.serialize())
    ));
    env.write_input(&data);

    env.assert_rejected("invalid existing approval signature");
}

#[test]
fn rejects_supplied_signatures_for_a_different_message_before_loading_wallet() {
    let env = SignTestEnv::new();
    let message = VersionedMessage::Legacy(approval_message(&[env.authority.pubkey()]));
    let mut data = sign_only(&message);
    data.signers.push(format!(
        "{}={}",
        env.authority.pubkey(),
        env.authority.sign_message(b"another message")
    ));
    env.write_input(&data);
    let original = fs::read(&env.input).unwrap();
    let missing_wallet = env.directory.path().join("missing-wallet.json");
    let mut config = SolanaConfig::load(&env.config_file_path).unwrap();
    config.keypair_path = missing_wallet.to_str().unwrap().to_string();
    config.save(&env.config_file_path).unwrap();
    let output = run_psigner_with_input(&env.args(&[]), "y\n");
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert!(
        stderr.contains("invalid existing approval signature"),
        "{stderr}"
    );
    assert!(!stderr.contains("Sign this approval?"));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&env.input).unwrap(), original);
}

#[test_case(false; "invalid signature last")]
#[test_case(true; "invalid signature first")]
fn rejects_invalid_duplicate_signatures(invalid_first: bool) {
    let env = SignTestEnv::new();
    let message = VersionedMessage::Legacy(approval_message(&[env.authority.pubkey()]));
    let mut data = sign_only(&message);
    data.signers = vec![
        format!(
            "{}={}",
            env.authority.pubkey(),
            env.authority.sign_message(&message.serialize())
        ),
        format!("{}={}", env.authority.pubkey(), Signature::default()),
    ];
    if invalid_first {
        data.signers.reverse();
    }
    env.write_input(&data);

    env.assert_rejected("invalid existing approval signature");
}

#[test]
fn verifies_and_counts_duplicate_signatures_once_without_updating_input() {
    let env = SignTestEnv::new();
    let message = VersionedMessage::Legacy(approval_message(&[env.authority.pubkey()]));
    let response = format!(
        "{}={}",
        env.authority.pubkey(),
        env.authority.sign_message(&message.serialize())
    );
    let mut data = sign_only(&message);
    data.signers = vec![response.clone(); 2];
    env.write_input(&data);
    let original = fs::read(&env.input).unwrap();

    let output = run_psigner(&env.args(&["--yes"]));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Approval signatures: 1 of 1 present"));
    assert!(stderr.contains(&format!(
        "{}: present (verified) (your signing key)",
        env.authority.pubkey()
    )));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{response}\n")
    );
    assert_eq!(fs::read(&env.input).unwrap(), original);
}

#[test_case(
    |msg| msg.instructions.clear(),
    "expected exactly one Execute instruction";
    "missing execute"
)]
#[test_case(
    |msg| msg.instructions.push(msg.instructions[0].clone()),
    "expected exactly one Execute instruction";
    "extra instruction"
)]
#[test_case(
    |msg| msg.instructions[0].program_id_index = 1,
    "expected the Legacy Message Executor";
    "wrong program"
)]
#[test_case(
    |msg| msg.instructions[0].program_id_index = u8::MAX,
    "invalid outer message";
    "invalid program index"
)]
#[test_case(
    |msg| msg.recent_blockhash = Hash::new_from_array([9; 32]),
    "outer message must use the default blockhash";
    "fee bearing blockhash"
)]
#[test_case(
    |msg| msg.instructions[0].data.clear(),
    "invalid Execute instruction";
    "invalid execute data"
)]
#[test_case(
    |msg| {
        let ExecutorInstruction::Execute(mut inner) =
            ExecutorInstruction::try_from_bytes(&msg.instructions[0].data).unwrap();
        inner.instructions[0].program_id_index = u8::MAX;
        msg.instructions[0].data = wincode::serialize(&ExecutorInstruction::Execute(inner)).unwrap();
    },
    "invalid inner message";
    "invalid inner account index"
)]
#[test_case(
    |msg| {
        let ExecutorInstruction::Execute(mut inner) =
            ExecutorInstruction::try_from_bytes(&msg.instructions[0].data).unwrap();
        inner.account_keys[1] = inner.account_keys[0];
        msg.instructions[0].data = wincode::serialize(&ExecutorInstruction::Execute(inner)).unwrap();
        msg.instructions[0].accounts[4] = msg.instructions[0].accounts[3];
    },
    "inner message must not contain duplicate account keys";
    "duplicate inner account keys"
)]
#[test_case(
    |msg| msg.instructions[0].accounts.truncate(2),
    "expected the nonce authority, nonce account, and SPL Nonce program";
    "missing nonce program"
)]
#[test_case(
    |msg| msg.instructions[0].accounts[2] = 0,
    "expected the SPL Nonce program as the third Execute account";
    "wrong nonce program"
)]
#[test_case(
    |msg| msg.instructions[0].accounts[3..].reverse(),
    "Execute accounts must mirror the inner message accounts";
    "reordered inner accounts"
)]
#[test_case(
    |msg| {
        msg.instructions[0].accounts.pop();
    },
    "Execute accounts must mirror the inner message accounts";
    "missing inner account"
)]
fn rejects_invalid_approval_messages(mutate: fn(&mut Message), expected: &str) {
    let env = SignTestEnv::new();
    let mut message = approval_message(&[env.authority.pubkey()]);
    mutate(&mut message);
    env.write_input(&sign_only(&VersionedMessage::Legacy(message)));

    env.assert_rejected(expected);
}

#[test]
fn rejects_execute_accounts_requiring_address_lookup_resolution() {
    let env = SignTestEnv::new();
    let mut message = approval_message(&[env.authority.pubkey()]);
    message.instructions[0].accounts[0] = u8::try_from(message.account_keys.len()).unwrap();
    let message = VersionedMessage::V0(v0::Message {
        header: message.header,
        account_keys: message.account_keys,
        recent_blockhash: message.recent_blockhash,
        instructions: message.instructions,
        address_table_lookups: vec![v0::MessageAddressTableLookup {
            account_key: Address::new_from_array([5; 32]),
            writable_indexes: vec![0],
            readonly_indexes: vec![],
        }],
    });
    env.write_input(&sign_only(&message));

    env.assert_rejected(
        "Execute accounts must use static account keys because ALTs are not resolved",
    );
}

#[test]
fn rejects_signer_outside_approval_authorities() {
    let env = SignTestEnv::new();
    let message = approval_message(&[Keypair::new_from_array([4; 32]).pubkey()]);
    env.write_input(&sign_only(&VersionedMessage::Legacy(message)));

    env.assert_rejected(&format!(
        "{} is not an approval authority for this message",
        env.authority.pubkey()
    ));
}

#[test]
fn refuses_to_emit_a_placeholder_signature_for_a_public_only_signer() {
    let env = SignTestEnv::new();
    let output = run_psigner_with_input(
        &env.args(&["--yes", "--signer", &env.authority.pubkey().to_string()]),
        "",
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing signature for supplied pubkey")
    );
}

#[test]
fn explicit_signer_selects_authority_independently_of_fee_payer() {
    let env = SignTestEnv::new();
    let authority = Keypair::new_from_array([4; 32]);
    let keypair_file = env.directory.path().join("override.json");
    write_keypair_file(&authority, &keypair_file).unwrap();
    let message = VersionedMessage::Legacy(approval_message(&[
        env.authority.pubkey(),
        authority.pubkey(),
    ]));
    env.write_input(&sign_only(&message));

    // An explicit signer must work with a missing config default
    // and an invalid fee-payer keypair.
    let default_keypair_file = env.directory.path().join("missing-default.json");
    let mut config = SolanaConfig::load(&env.config_file_path).unwrap();
    config.keypair_path = default_keypair_file.to_str().unwrap().to_string();
    config.save(&env.config_file_path).unwrap();
    let fee_payer_file = env.directory.path().join("invalid-fee-payer.json");
    fs::write(&fee_payer_file, "not a keypair").unwrap();

    let output = run_psigner(&env.args(&[
        "--yes",
        "--signer",
        keypair_file.to_str().unwrap(),
        "--fee-payer",
        fee_payer_file.to_str().unwrap(),
    ]));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}={}\n",
            authority.pubkey(),
            authority.sign_message(&message.serialize())
        )
    );
}

#[test_case("y\n", true; "short yes")]
#[test_case("yes\n", true; "yes")]
#[test_case("  YES  \n", true; "case insensitive trimmed yes")]
#[test_case("n\n", false; "explicit no")]
#[test_case("\n", false; "default no")]
#[test_case("yes please\n", false; "unrecognized answer")]
#[test_case("", false; "end of input")]
fn requires_confirmation(answer: &str, approved: bool) {
    let env = SignTestEnv::new();
    let original = fs::read(&env.input).unwrap();
    let output = run_psigner_with_input(&env.args(&[]), answer);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(stderr.contains("Sign this approval? [y/N] "), "{stderr}");
    assert_eq!(output.status.success(), approved, "{stderr}");
    assert_eq!(fs::read(&env.input).unwrap(), original);
    if approved {
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}={}\n",
                env.authority.pubkey(),
                env.authority
                    .sign_message(&approval_message(&[env.authority.pubkey()]).serialize())
            )
        );
    } else {
        assert!(stderr.contains("signing cancelled"), "{stderr}");
        assert!(output.stdout.is_empty());
    }
}
