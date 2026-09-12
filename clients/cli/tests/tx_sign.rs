//! Real offline `tx sign` tests using shared sign-only reports and SDK transactions,
//! without RPC requests.

use {
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_config::Config as SolanaConfig,
    solana_cli_output::{
        CliSignOnlyData, ReturnSignersConfig, parse_sign_only_reply_string, return_signers_data,
    },
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::{VersionedMessage, legacy::Message, v0, v1},
    solana_sanitize::Sanitize,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_system_interface::instruction::transfer,
    solana_transaction::{Transaction, versioned::VersionedTransaction},
    spl_ed25519_signer_client::{
        ProgrammaticSigner, instruction::submit, message::wrapped_message,
    },
    spl_legacy_message_executor_client::instruction::execute,
    spl_legacy_message_executor_interface::instruction::Instruction as ExecutorInstruction,
    std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::{Command, Output, Stdio},
    },
    tempfile::TempDir,
};

fn address(byte: u8) -> Address {
    Address::new_from_array([byte; 32])
}

fn pda(authority: &Address) -> Address {
    ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority)
}

fn envelope_with_instructions(
    authorities: &[Address],
    instructions: &[Instruction],
) -> Transaction {
    let inner = Message::new_with_blockhash(
        instructions,
        Some(&pda(&authorities[0])),
        &Hash::new_from_array([8; 32]),
    );
    let mut message = wrapped_message(&execute(&address(2), &inner), authorities);
    message.set_recent_blockhash(Hash::new_from_array([9; 32]));
    let VersionedMessage::Legacy(message) = message else {
        unreachable!()
    };
    Transaction::new_unsigned(message)
}

fn unsigned(authorities: &[Address]) -> Transaction {
    let instructions = authorities
        .iter()
        .map(|authority| transfer(&pda(authority), &address(3), 1))
        .collect::<Vec<_>>();
    envelope_with_instructions(authorities, &instructions)
}

fn sign_only(transaction: &Transaction) -> CliSignOnlyData {
    return_signers_data(
        transaction,
        &ReturnSignersConfig {
            dump_transaction_message: true,
        },
    )
}

fn approval_message_versions(authorities: &[Address]) -> [(&'static str, VersionedMessage); 4] {
    let legacy = unsigned(authorities).message;
    let v0 = v0::Message {
        header: legacy.header,
        account_keys: legacy.account_keys.clone(),
        recent_blockhash: legacy.recent_blockhash,
        instructions: legacy.instructions.clone(),
        address_table_lookups: vec![],
    };
    let mut unused_lookups = v0.clone();
    unused_lookups
        .address_table_lookups
        .push(v0::MessageAddressTableLookup {
            account_key: address(80),
            writable_indexes: vec![1],
            readonly_indexes: vec![2],
        });
    let v1 = v1::Message::new(
        legacy.header,
        v1::TransactionConfig::empty()
            .with_priority_fee(123)
            .with_compute_unit_limit(123_456)
            .with_loaded_accounts_data_size_limit(32_768)
            .with_heap_size(65_536),
        legacy.recent_blockhash,
        legacy.account_keys.clone(),
        legacy.instructions.clone(),
    );
    [
        ("legacy", VersionedMessage::Legacy(legacy)),
        ("v0", VersionedMessage::V0(v0)),
        ("v0-unused-lookups", VersionedMessage::V0(unused_lookups)),
        ("v1", VersionedMessage::V1(v1)),
    ]
}

// A producer can use the shared JSON schema without the legacy-only export helper.
fn versioned_sign_only(message: &VersionedMessage, signers: &[&dyn Signer]) -> CliSignOnlyData {
    let bytes = message.serialize();
    CliSignOnlyData {
        blockhash: message.recent_blockhash().to_string(),
        message: Some(BASE64_STANDARD.encode(&bytes)),
        signers: signers
            .iter()
            .map(|signer| format!("{}={}", signer.pubkey(), signer.sign_message(&bytes)))
            .collect(),
        ..CliSignOnlyData::default()
    }
}

fn read_transaction(path: &Path) -> Transaction {
    let json = fs::read_to_string(path).unwrap();
    // Exercise the upstream consumer as well as the shared output schema.
    let data = parse_sign_only_reply_string(&json);
    let bytes = BASE64_STANDARD
        .decode(data.message.as_ref().unwrap())
        .unwrap();
    let message: Message = wincode::deserialize_exact(&bytes).unwrap();
    let mut transaction = Transaction::new_unsigned(message);
    let presigners = transaction
        .message
        .signer_keys()
        .into_iter()
        .filter_map(|key| data.presigner_of(key))
        .collect::<Vec<_>>();
    transaction
        .try_partial_sign(&presigners, data.blockhash)
        .unwrap();
    assert_eq!(transaction.message_data(), bytes);
    assert_eq!(
        serde_json::from_str::<CliSignOnlyData>(&json).unwrap(),
        sign_only(&transaction)
    );
    transaction
}

struct CliFixture {
    directory: TempDir,
    input: PathBuf,
    key: PathBuf,
    config: PathBuf,
    authority: Keypair,
}

impl CliFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let authority = Keypair::new();
        let input = directory.path().join("input.json");
        let key = directory.path().join("authority.json");
        let config = directory.path().join("config.yml");
        write_keypair_file(&authority, &key).unwrap();
        // Valid configuration with an unreachable endpoint: signing must not make RPC requests.
        SolanaConfig {
            json_rpc_url: "http://127.0.0.1:1".to_string(),
            commitment: "confirmed".to_string(),
            keypair_path: key.to_str().unwrap().to_string(),
            ..SolanaConfig::default()
        }
        .save(config.to_str().unwrap())
        .unwrap();
        fs::write(
            &input,
            serde_json::to_vec(&sign_only(&unsigned(&[authority.pubkey()]))).unwrap(),
        )
        .unwrap();
        Self {
            directory,
            input,
            key,
            config,
            authority,
        }
    }

    fn output(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"));
        command.arg("--config").arg(&self.config);
        command
    }

    fn command(&self, output: &Path) -> Command {
        let mut command = self.cli();
        command
            .args(["tx", "sign"])
            .arg(&self.input)
            .arg("--outfile")
            .arg(output);
        command
    }
}

#[test]
fn cli_uses_the_configured_keypair_for_offline_signing() {
    let fixture = CliFixture::new();
    for flag in ["--config", "-C"] {
        for position in [0, 1, 2, 5] {
            let path = fixture.output(&format!("configured-{flag}-{position}.json"));
            let mut args = vec![
                "tx",
                "sign",
                fixture.input.to_str().unwrap(),
                "--outfile",
                path.to_str().unwrap(),
            ];
            args.splice(position..position, [flag, fixture.config.to_str().unwrap()]);
            let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
                .args(&args)
                .output()
                .unwrap();
            assert_success(&output);
            let signed = read_transaction(&path);
            assert_eq!(
                signed.signatures[0],
                fixture.authority.sign_message(&signed.message_data())
            );
        }
    }
}

#[test]
fn cli_accepts_keypair_overrides_at_every_global_argument_position() {
    let fixture = CliFixture::new();
    let override_key = fixture.output("override.json");
    write_keypair_file(&fixture.authority, &override_key).unwrap();
    fs::write(&fixture.key, "must not load the configured keypair").unwrap();
    for flag in ["--keypair", "-k"] {
        for position in [0, 1, 2, 5] {
            let path = fixture.output(&format!("overridden-{flag}-{position}.json"));
            let mut args = vec![
                "tx",
                "sign",
                fixture.input.to_str().unwrap(),
                "--outfile",
                path.to_str().unwrap(),
            ];
            args.splice(position..position, [flag, override_key.to_str().unwrap()]);
            let output = fixture.cli().args(&args).output().unwrap();
            assert_success(&output);
            let signed = read_transaction(&path);
            assert_eq!(
                signed.signatures[0],
                fixture.authority.sign_message(&signed.message_data())
            );
        }
    }
}

#[test]
fn cli_rejects_missing_or_malformed_explicit_config_even_with_a_keypair_override() {
    let fixture = CliFixture::new();
    let malformed = fixture.output("malformed.yml");
    fs::write(&malformed, "keypair_path: [unterminated").unwrap();
    let missing = fixture.output("missing.yml");
    let path = fixture.output("signed.json");
    for config in [missing, malformed] {
        let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
            .arg("--config")
            .arg(&config)
            .arg("--keypair")
            .arg(&fixture.key)
            .args(["tx", "sign"])
            .arg(&fixture.input)
            .arg("--outfile")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("failed to load Solana CLI config"),
            "{error}"
        );
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn cli_does_not_use_fee_payer_as_the_default_approval_authority() {
    let fixture = CliFixture::new();
    let fee_payer = fixture.output("fee-payer.json");
    write_keypair_file(&Keypair::new(), &fee_payer).unwrap();
    let path = fixture.output("signed.json");
    let output = fixture
        .command(&path)
        .arg("--fee-payer")
        .arg(fee_payer)
        .output()
        .unwrap();
    assert_success(&output);
    let signed = read_transaction(&path);
    assert_eq!(
        signed.signatures[0],
        fixture.authority.sign_message(&signed.message_data())
    );
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_accepts_explicit_typed_key_sources_without_opening_wallets() {
    let fixture = CliFixture::new();
    // A malformed artifact stops execution after parsing but before any wallet access.
    fs::write(&fixture.input, "not JSON").unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = fixture.output("signed.json");
    let authority_address = fixture.authority.pubkey().to_string();
    let base58_keypair = fixture.authority.to_base58_string();
    // An empty file is a valid source at parse time; it is not a valid loaded keypair.
    for source in [
        file.path().to_str().unwrap(),
        "usb://ledger?key=0/0",
        "prompt://?key=0/0",
        "ASK",
        "-",
        "stdin:",
        &authority_address,
        &base58_keypair,
    ] {
        let output = fixture
            .command(&path)
            .args(["--keypair", source])
            .output()
            .unwrap();
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(1), "{source}: {error}");
        assert!(
            error.contains("expected ident at line 1 column 2"),
            "{source}: {error}"
        );
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn cli_signs_with_a_base58_keypair_source() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "must not load the configured keypair").unwrap();
    let path = fixture.output("signed.json");
    let output = fixture
        .command(&path)
        .args(["--keypair", &fixture.authority.to_base58_string()])
        .output()
        .unwrap();
    assert_success(&output);
    let signed = read_transaction(&path);
    assert_eq!(
        signed.signatures[0],
        fixture.authority.sign_message(&signed.message_data())
    );
}

#[test]
fn cli_requires_a_signature_for_address_sources() {
    let fixture = CliFixture::new();
    let path = fixture.output("signed.json");
    let output = fixture
        .command(&path)
        .args(["-k", &fixture.authority.pubkey().to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("missing signature for supplied pubkey"),
        "{error}"
    );
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
}

#[test]
fn cli_does_not_accept_presigner_or_outer_modes() {
    let fixture = CliFixture::new();
    let path = fixture.output("signed.json");
    for unsupported in ["--signer", "--sign-only", "--outer", "--blockhash"] {
        let output = fixture.command(&path).arg(unsupported).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{unsupported}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn cli_requires_transaction_and_output_even_with_global_options() {
    let fixture = CliFixture::new();
    let path = fixture.output("signed.json");
    for missing in [2..3, 5..7] {
        for flags in [
            vec![],
            vec!["--config", "/missing/config.yml"],
            vec!["--url", "localhost"],
            vec!["--commitment", "confirmed"],
            vec!["--fee-payer", "/missing/hot-keypair.json"],
            vec!["--skip-preflight"],
        ] {
            let mut args = vec![
                "tx",
                "sign",
                fixture.input.to_str().unwrap(),
                "-k",
                "usb://ledger",
                "--outfile",
                path.to_str().unwrap(),
            ];
            args.drain(missing.clone());
            args.extend(flags);
            let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
                .args(&args)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(!path.exists());
        }
    }
}

#[test]
fn cli_accepts_output_and_seed_phrase_options_at_every_global_argument_position() {
    let fixture = CliFixture::new();
    fs::write(&fixture.input, "not JSON").unwrap();
    let path = fixture.output("signed.json");
    for position in [0, 1, 2, 7] {
        let mut args = vec![
            "tx",
            "sign",
            fixture.input.to_str().unwrap(),
            "-k",
            "prompt://",
            "--outfile",
            path.to_str().unwrap(),
        ];
        args.splice(
            position..position,
            ["--output", "json", "--skip-seed-phrase-validation"],
        );
        let output = fixture.cli().args(&args).output().unwrap();
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {error}");
        assert!(
            error.contains("expected ident at line 1 column 2"),
            "{args:?}: {error}"
        );
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn cli_signs_offline_in_all_output_formats_and_preserves_input() {
    let fixture = CliFixture::new();
    let original = fs::read(&fixture.input).unwrap();
    for format in ["display", "json", "json-compact"] {
        let path = fixture.output(format);
        let output = fixture
            .command(&path)
            .args(["--output", format])
            .output()
            .unwrap();
        assert_success(&output);
        let review = String::from_utf8(output.stderr).unwrap();
        assert!(review.contains("No separate Submit relay is constructed or signed"));
        assert!(review.contains(&fixture.authority.pubkey().to_string()));
        let signed = read_transaction(&path);
        let report = sign_only(&signed);
        let data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if format == "display" {
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{report}\n")
            );
        } else {
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(json, data);
            assert_eq!(
                serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
                report
            );
            if format == "json-compact" {
                assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
            }
        }
        let input: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(data["message"], input["message"]);
        assert_eq!(data["blockhash"], input["blockhash"]);
        assert_eq!(
            data["signers"],
            serde_json::json!([format!(
                "{}={}",
                fixture.authority.pubkey(),
                signed.signatures[0]
            )])
        );
        assert!(data.get("signatures").is_none());
        assert!(data.get("absent").is_none());
        assert!(data.get("badSig").is_none());
        assert_eq!(
            signed.signatures[0],
            fixture.authority.sign_message(&signed.message_data())
        );
        assert_eq!(fs::read(&fixture.input).unwrap(), original);
    }
}

#[test]
fn cli_passes_partial_artifacts_between_authorities_without_touching_the_message() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    let second_key = fixture.output("second-key.json");
    write_keypair_file(&second, &second_key).unwrap();
    let initial = unsigned(&[fixture.authority.pubkey(), second.pubkey()]);
    for (order, keys) in [
        [(&fixture.key, &fixture.authority), (&second_key, &second)],
        [(&second_key, &second), (&fixture.key, &fixture.authority)],
    ]
    .into_iter()
    .enumerate()
    {
        let mut expected = initial.clone();
        fs::write(
            &fixture.input,
            serde_json::to_vec(&sign_only(&initial)).unwrap(),
        )
        .unwrap();
        let mut input = fixture.input.clone();
        for (stage, (key_path, signer)) in keys.into_iter().enumerate() {
            let before = fs::read(&input).unwrap();
            let path = fixture.output(&format!("signed-{order}-{stage}.json"));
            let output = fixture
                .cli()
                .args(["tx", "sign"])
                .arg(&input)
                .arg("-k")
                .arg(key_path)
                .arg("--outfile")
                .arg(&path)
                .args(["--output", "json"])
                .output()
                .unwrap();
            assert_success(&output);
            expected
                .try_partial_sign(&[signer], expected.message.recent_blockhash)
                .unwrap();
            assert_eq!(read_transaction(&path), expected);
            assert_eq!(
                serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
                sign_only(&expected)
            );
            assert_eq!(expected.message, initial.message);
            assert_eq!(fs::read(&input).unwrap(), before);
            // Exercise both supported JSON handoffs: the output file and captured stdout.
            input = if order == 0 {
                path
            } else {
                let stdout_path = fixture.output(&format!("stdout-{order}-{stage}.json"));
                fs::write(&stdout_path, &output.stdout).unwrap();
                stdout_path
            };
        }
    }
}

#[test]
fn cli_online_options_do_not_change_the_signed_transaction() {
    let fixture = CliFixture::new();
    let original = fs::read(&fixture.input).unwrap();
    let mut expected = read_transaction(&fixture.input);
    expected.signatures[0] = fixture.authority.sign_message(&expected.message_data());
    let missing_fee_payer = fixture.output("missing-fee-payer.json");
    let fee_payer = missing_fee_payer.to_str().unwrap();
    for (case, flags) in [
        vec!["--url", "http://127.0.0.1:1"],
        vec!["-u", "localhost"],
        vec!["--fee-payer", fee_payer],
        vec!["--commitment", "confirmed"],
        vec!["--skip-preflight"],
        vec![
            "--url",
            "http://127.0.0.1:1",
            "--fee-payer",
            fee_payer,
            "--commitment",
            "finalized",
            "--skip-preflight",
        ],
    ]
    .into_iter()
    .enumerate()
    {
        for position in [0, 1, 2, 7] {
            let path = fixture.output(&format!("signed-{case}-{position}.json"));
            let mut args = vec![
                "tx",
                "sign",
                fixture.input.to_str().unwrap(),
                "--keypair",
                fixture.key.to_str().unwrap(),
                "--outfile",
                path.to_str().unwrap(),
            ];
            args.splice(position..position, flags.iter().copied());
            let output = fixture.cli().args(&args).output().unwrap();
            assert_success(&output);
            assert_eq!(read_transaction(&path), expected);
            assert_eq!(fs::read(&fixture.input).unwrap(), original);
        }
    }
}

#[test]
fn cli_uses_tx_without_a_transaction_alias() {
    let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
        .arg("--help")
        .output()
        .unwrap();
    assert_success(&output);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(
        help.lines()
            .any(|line| line.split_whitespace().next() == Some("tx")),
        "{help}"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
        .args(["transaction", "sign", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("Found argument 'transaction'"), "{error}");
}

#[test]
fn cli_help_labels_the_shared_sign_only_artifact_and_offline_network_behavior() {
    let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
        .args(["tx", "sign", "--help"])
        .output()
        .unwrap();
    assert_success(&output);
    let help = String::from_utf8(output.stdout).unwrap();
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(help.contains(" tx sign "), "{help}");
    assert!(help.contains("<SIGN_ONLY_FILE>"), "{help}");
    assert!(help.contains("--outfile <FILEPATH>"), "{help}");
    assert!(help.contains("CliSignOnlyData"), "{help}");
    assert!(!help.contains("SDK legacy Transaction JSON"), "{help}");
    assert!(
        help.contains(
            "Default signer source. Overrides the keypair in the Solana CLI configuration"
        ),
        "{help}"
    );
    assert!(
        help.contains("without RPC requests or a Submit relay"),
        "{help}"
    );
    assert!(
        help.contains("Supports legacy, v0, and v1 approvals using static executor accounts"),
        "{help}"
    );
    assert!(
        help.contains("Write updated `CliSignOnlyData` JSON, including the message"),
        "{help}"
    );
    assert!(!help.contains("Does not sign outer transactions"), "{help}");
}

#[test]
fn cli_rejects_invalid_input_before_loading_a_wallet() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "not a keypair").unwrap();
    fs::write(&fixture.input, "not JSON").unwrap();
    let path = fixture.output("invalid.json");
    let output = fixture.command(&path).output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("expected ident at line 1 column 2"),
        "{error}"
    );
    assert!(!error.contains("invalid artifact JSON"), "{error}");
    assert!(!error.contains("failed to load approval authority"));
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
}

#[test]
fn cli_rejects_non_authorities_without_creating_a_signed_artifact() {
    let fixture = CliFixture::new();
    let non_authority = Keypair::new();
    let transaction = envelope_with_instructions(
        &[fixture.authority.pubkey()],
        &[transfer(
            &pda(&fixture.authority.pubkey()),
            &non_authority.pubkey(),
            1,
        )],
    );
    // An account's presence in the message must not make it an approval signer.
    let original = serde_json::to_vec(&sign_only(&transaction)).unwrap();
    fs::write(&fixture.input, &original).unwrap();
    write_keypair_file(&non_authority, &fixture.key).unwrap();
    let path = fixture.output("wrong-key.json");
    let output = fixture.command(&path).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("keypair-pubkey mismatch")
    );
    assert_eq!(fs::read(&fixture.input).unwrap(), original);
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
}

#[test]
fn cli_accepts_keypair_stdin_without_using_stdin_for_the_artifact() {
    let fixture = CliFixture::new();
    let path = fixture.output("stdin-signed.json");
    let mut child = fixture
        .command(&path)
        .args(["-k", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&fs::read(&fixture.key).unwrap())
        .unwrap();
    assert_success(&child.wait_with_output().unwrap());
    let signed = read_transaction(&path);
    assert_ne!(signed.signatures[0], Signature::default());
}

#[test]
fn cli_rejects_an_outer_submit_before_attempting_to_load_the_wallet() {
    let fixture = CliFixture::new();
    let envelope = unsigned(&[fixture.authority.pubkey()]);
    let outer = Transaction::new_unsigned(Message::new_with_blockhash(
        &[submit(
            envelope.signatures,
            VersionedMessage::Legacy(envelope.message),
        )],
        Some(&fixture.authority.pubkey()),
        &Hash::new_from_array([14; 32]),
    ));
    fs::write(
        &fixture.input,
        serde_json::to_vec(&sign_only(&outer)).unwrap(),
    )
    .unwrap();
    fs::write(&fixture.key, "must not load").unwrap();
    let path = fixture.output("outer.json");
    let output = fixture.command(&path).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Submit relays are not accepted")
    );
    assert!(!path.exists());
}

#[test]
fn cli_signs_only_the_sdk_message_not_the_relay() {
    let fixture = CliFixture::new();
    let initial = read_transaction(&fixture.input);
    let path = fixture.output("signed.json");
    assert_success(&fixture.command(&path).output().unwrap());
    let signed = read_transaction(&path);
    let expected_bytes = VersionedMessage::Legacy(initial.message.clone()).serialize();
    assert_eq!(signed.message, initial.message);
    assert_eq!(
        signed.signatures,
        vec![fixture.authority.sign_message(&expected_bytes)]
    );
    assert!(signed.signatures[0].verify(fixture.authority.pubkey().as_ref(), &expected_bytes));

    let hot = Keypair::new();
    let relay = Transaction::new_unsigned(Message::new_with_blockhash(
        &[submit(
            signed.signatures.clone(),
            VersionedMessage::Legacy(signed.message.clone()),
        )],
        Some(&hot.pubkey()),
        &Hash::new_from_array([10; 32]),
    ));
    assert!(
        relay
            .signatures
            .iter()
            .all(|signature| *signature == Signature::default())
    );
    assert_ne!(relay.message_data(), expected_bytes);
    assert!(
        !signed.signatures[0].verify(fixture.authority.pubkey().as_ref(), &relay.message_data())
    );
}

#[test]
fn cli_accepts_execution_preflight_failures_without_rebuilding_the_message() {
    let fixture = CliFixture::new();
    let authority = fixture.authority.pubkey();
    let valid = unsigned(&[authority]);
    let mut noncanonical = valid.clone();
    noncanonical.message.account_keys.push(address(90));
    noncanonical.message.header.num_readonly_unsigned_accounts = noncanonical
        .message
        .header
        .num_readonly_unsigned_accounts
        .saturating_add(1);
    let uncovered =
        envelope_with_instructions(&[authority], &[transfer(&address(91), &address(3), 1)]);
    let additional_role = envelope_with_instructions(
        &[authority],
        &[
            transfer(&pda(&authority), &address(3), 1),
            transfer(&authority, &address(3), 1),
        ],
    );
    let mut instruction = transfer(&pda(&authority), &address(3), 1);
    instruction.data.resize(1233, 0);
    let oversized = envelope_with_instructions(&[authority], &[instruction]);
    let mut duplicates = valid.clone();
    let ExecutorInstruction::Execute(mut inner) =
        ExecutorInstruction::try_from_bytes(&duplicates.message.instructions[0].data).unwrap();
    inner.account_keys[1] = inner.account_keys[0];
    duplicates.message.instructions[0].data =
        wincode::serialize(&ExecutorInstruction::Execute(inner)).unwrap();

    for (name, transaction) in [
        ("noncanonical", noncanonical),
        ("uncovered", uncovered),
        ("additional-role", additional_role),
        ("oversized", oversized),
        ("duplicates", duplicates),
    ] {
        transaction.sanitize().unwrap();
        let bytes = serde_json::to_vec(&sign_only(&transaction)).unwrap();
        fs::write(&fixture.input, &bytes).unwrap();
        let path = fixture.output(name);
        let output = fixture
            .command(&path)
            .args(["--output", "json"])
            .output()
            .unwrap();
        assert_success(&output);
        let signed = read_transaction(&path);
        let mut expected = transaction;
        expected
            .try_partial_sign(&[&fixture.authority], expected.message.recent_blockhash)
            .unwrap();
        assert_eq!(signed, expected, "{name}");
        assert_eq!(fs::read(&fixture.input).unwrap(), bytes);
        assert_eq!(
            serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
            sign_only(&expected)
        );
    }
}

#[test]
fn cli_preserves_verified_signatures_when_signing_again() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    let mut initial = unsigned(&[fixture.authority.pubkey(), second.pubkey()]);
    initial
        .try_partial_sign(
            &[&fixture.authority, &second],
            initial.message.recent_blockhash,
        )
        .unwrap();
    let mut data = sign_only(&initial);
    // Entries are keyed by address, not by their position in the report.
    data.signers.reverse();
    let bytes = serde_json::to_vec(&data).unwrap();
    fs::write(&fixture.input, &bytes).unwrap();
    let path = fixture.output("signed.json");
    let output = fixture.command(&path).output().unwrap();
    assert_success(&output);
    let signed = read_transaction(&path);
    assert_eq!(signed.message, initial.message);
    assert_eq!(signed.signatures[1], initial.signatures[1]);
    assert_eq!(
        signed.signatures[0],
        fixture.authority.sign_message(&initial.message_data())
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("present (verified)")
    );
    assert_eq!(fs::read(&fixture.input).unwrap(), bytes);
}

#[test]
fn cli_recomputes_status_instead_of_trusting_report_summaries() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    let third = Keypair::new();
    let mut transaction = unsigned(&[fixture.authority.pubkey(), second.pubkey(), third.pubkey()]);
    transaction
        .try_partial_sign(&[&second], transaction.message.recent_blockhash)
        .unwrap();
    let mut data = sign_only(&transaction);
    // These lists do not describe the actual signatures. They cannot hide an absent signer,
    // discard an existing signature, or grant approval authority to an unrelated account.
    data.absent = vec![second.pubkey().to_string(), address(99).to_string()];
    data.bad_sig = vec![
        fixture.authority.pubkey().to_string(),
        second.pubkey().to_string(),
    ];
    fs::write(&fixture.input, serde_json::to_vec(&data).unwrap()).unwrap();
    let path = fixture.output("corrected.json");
    let output = fixture
        .command(&path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert_success(&output);
    transaction
        .try_partial_sign(&[&fixture.authority], transaction.message.recent_blockhash)
        .unwrap();
    assert_eq!(read_transaction(&path), transaction);
    let result: CliSignOnlyData = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(result.signers.len(), 2);
    assert_eq!(result.absent, vec![third.pubkey().to_string()]);
    assert!(result.bad_sig.is_empty());
    assert_eq!(
        serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
        result
    );
    let review = String::from_utf8(output.stderr).unwrap();
    assert!(review.contains(&format!("{}: missing", third.pubkey())));
    assert!(review.contains(&format!("{}: present (verified)", second.pubkey())));
}

#[test]
fn cli_accepts_minimal_unsigned_reports_without_status_lists() {
    let fixture = CliFixture::new();
    let transaction = unsigned(&[fixture.authority.pubkey()]);
    fs::write(
        &fixture.input,
        serde_json::to_vec(&serde_json::json!({
            "blockhash": transaction.message.recent_blockhash.to_string(),
            "message": BASE64_STANDARD.encode(transaction.message_data()),
        }))
        .unwrap(),
    )
    .unwrap();
    let path = fixture.output("signed.json");
    assert_success(&fixture.command(&path).output().unwrap());
    let signed = read_transaction(&path);
    assert_eq!(signed.message, transaction.message);
    assert_eq!(
        signed.signatures[0],
        fixture.authority.sign_message(&transaction.message_data())
    );
}

#[test]
fn cli_rejects_invalid_sign_only_payloads_before_loading_a_wallet() {
    let fixture = CliFixture::new();
    let transaction = unsigned(&[fixture.authority.pubkey()]);
    let valid = serde_json::to_value(sign_only(&transaction)).unwrap();
    let mut no_message = valid.clone();
    no_message.as_object_mut().unwrap().remove("message");
    let message_bytes = transaction.message_data();
    let mut trailing = message_bytes.clone();
    trailing.push(0);
    let mut aliased = message_bytes.clone();
    // A noncanonical short-vector length must not be silently reserialized before signing.
    assert!(aliased[3] < 128);
    aliased[3] |= 128;
    aliased.insert(4, 0);
    let foreign = Keypair::new();
    let key = fixture.authority.pubkey();
    let signature = fixture.authority.sign_message(&message_bytes);
    let mut cases = vec![("missing-message", no_message, "missing transaction message")];
    for (name, field, value, error) in [
        (
            "null-message",
            "message",
            serde_json::Value::Null,
            "missing transaction message",
        ),
        (
            "bad-base64",
            "message",
            serde_json::json!("!"),
            "invalid base64 message",
        ),
        (
            "empty-message",
            "message",
            serde_json::json!(""),
            "invalid serialized message",
        ),
        (
            "truncated-message",
            "message",
            serde_json::json!(BASE64_STANDARD.encode(&message_bytes[..10])),
            "invalid serialized message",
        ),
        (
            "trailing-message",
            "message",
            serde_json::json!(BASE64_STANDARD.encode(trailing)),
            "invalid serialized message",
        ),
        (
            "aliased-length",
            "message",
            serde_json::json!(BASE64_STANDARD.encode(aliased)),
            "invalid serialized message",
        ),
        (
            "missing-separator",
            "signers",
            serde_json::json!([key.to_string()]),
            "expected ADDRESS=SIGNATURE",
        ),
        (
            "malformed-address",
            "signers",
            serde_json::json!([format!("bad={signature}")]),
            "invalid signer address",
        ),
        (
            "malformed-signature",
            "signers",
            serde_json::json!([format!("{key}=bad")]),
            "invalid signer signature",
        ),
        (
            "extra-separator",
            "signers",
            serde_json::json!([format!("{key}={signature}=extra")]),
            "invalid signer signature",
        ),
        (
            "zero-signature",
            "signers",
            serde_json::json!([format!("{key}={}", Signature::default())]),
            "invalid existing approval signature",
        ),
        (
            "invalid-signature",
            "signers",
            serde_json::json!([format!("{key}={}", Signature::from([5; 64]))]),
            "invalid existing approval signature",
        ),
        (
            "shadowed-invalid-signature",
            "signers",
            serde_json::json!([
                format!("{key}={}", Signature::from([5; 64])),
                format!("{key}={signature}")
            ]),
            "invalid existing approval signature",
        ),
        (
            "different-message",
            "signers",
            serde_json::json!([format!(
                "{key}={}",
                fixture.authority.sign_message(b"different message")
            )]),
            "invalid existing approval signature",
        ),
        (
            "foreign-signer",
            "signers",
            serde_json::json!([format!(
                "{}={}",
                foreign.pubkey(),
                foreign.sign_message(&message_bytes)
            )]),
            "invalid existing approval signature",
        ),
    ] {
        let mut data = valid.clone();
        data[field] = value;
        cases.push((name, data, error));
    }
    // The former SDK JSON is deliberately not another supported interchange schema.
    cases.push((
        "sdk-json",
        serde_json::to_value(&transaction).unwrap(),
        "invalid type: map, expected a string",
    ));
    fs::write(&fixture.key, "must not load").unwrap();
    for (name, data, expected_error) in cases {
        let original = serde_json::to_vec(&data).unwrap();
        fs::write(&fixture.input, &original).unwrap();
        let path = fixture.output(name);
        let output = fixture.command(&path).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(expected_error), "{name}: {error}");
        assert!(
            !error.contains("failed to load approval authority"),
            "{name}: {error}"
        );
        assert!(!error.contains("panicked"), "{name}: {error}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(!path.exists(), "{name}");
        assert_eq!(fs::read(&fixture.input).unwrap(), original, "{name}");
    }
}

#[test]
fn cli_rejects_uninspectable_or_unrelated_transactions_before_loading_the_wallet() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "must not load").unwrap();
    let valid = unsigned(&[fixture.authority.pubkey()]);
    let mut zero_signers = valid.clone();
    zero_signers.message.header.num_required_signatures = 0;
    zero_signers.signatures.clear();
    let mut missing_authorities = valid.clone();
    missing_authorities.message.header.num_required_signatures = 127;
    let mut bad_index = valid.clone();
    bad_index.message.instructions[0].accounts[0] = 255;
    let mut bad_program = valid.clone();
    bad_program.message.instructions[0].program_id_index = 255;
    let mut missing_instruction = valid.clone();
    missing_instruction.message.instructions.clear();
    let mut extra_instruction = valid.clone();
    extra_instruction
        .message
        .instructions
        .push(valid.message.instructions[0].clone());
    let mut unknown_tag = valid.clone();
    unknown_tag.message.instructions[0].data[0] = 255;
    let mut trailing = valid.clone();
    trailing.message.instructions[0].data.push(0);
    let mut missing_nonce = valid.clone();
    missing_nonce.message.instructions[0].accounts.clear();
    let mut bad_inner = valid;
    let ExecutorInstruction::Execute(mut inner) =
        ExecutorInstruction::try_from_bytes(&bad_inner.message.instructions[0].data).unwrap();
    inner.instructions[0].accounts[0] = 255;
    bad_inner.message.instructions[0].data =
        wincode::serialize(&ExecutorInstruction::Execute(inner)).unwrap();
    let ordinary = Transaction::new_unsigned(Message::new(
        &[transfer(&fixture.authority.pubkey(), &address(3), 1)],
        Some(&fixture.authority.pubkey()),
    ));

    for (name, transaction) in [
        ("zero-signers", zero_signers),
        ("missing-authorities", missing_authorities),
        ("account-index", bad_index),
        ("program-index", bad_program),
        ("missing-instruction", missing_instruction),
        ("extra-instruction", extra_instruction),
        ("unknown-tag", unknown_tag),
        ("trailing-data", trailing),
        ("missing-nonce", missing_nonce),
        ("inner-index", bad_inner),
        ("ordinary-transfer", ordinary),
    ] {
        let bytes = serde_json::to_vec(&sign_only(&transaction)).unwrap();
        fs::write(&fixture.input, &bytes).unwrap();
        let path = fixture.output(name);
        let output = fixture.command(&path).output().unwrap();
        assert!(!output.status.success(), "{name}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            !error.contains("failed to load approval authority"),
            "{name}: {error}"
        );
        assert!(!error.contains("panicked"), "{name}: {error}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(!path.exists(), "{name}");
        assert_eq!(fs::read(&fixture.input).unwrap(), bytes);
    }
}

#[test]
fn cli_signs_an_inspectable_message_without_transaction_sanitization() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    let mut message = unsigned(&[fixture.authority.pubkey(), second.pubkey()]).message;
    // Outer transaction validity is not a prerequisite for signing the supplied bytes.
    // These flags do not prevent inspecting the Execute instruction and its inner message.
    message.header.num_readonly_unsigned_accounts = u8::MAX;
    assert!(message.sanitize().is_err());
    let message = VersionedMessage::Legacy(message);
    let report = versioned_sign_only(&message, &[&second]);
    let input_bytes = serde_json::to_vec(&report).unwrap();
    fs::write(&fixture.input, &input_bytes).unwrap();
    let path = fixture.output("signed-without-sanitization.json");
    let output = fixture.command(&path).output().unwrap();
    assert_success(&output);
    let result: CliSignOnlyData = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        result,
        versioned_sign_only(&message, &[&fixture.authority, &second])
    );
    assert_eq!(result.message, report.message);
    assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
    let review = String::from_utf8(output.stderr).unwrap();
    assert!(review.contains(&VersionedMessage::hash_raw_message(&message.serialize()).to_string()));
    assert!(review.contains(
        "execution validity, cluster identity, live nonce state and business intent are not \
         verified"
    ));
}

#[test]
fn cli_signs_all_message_versions_without_changing_bytes_or_other_approvals() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    let third = Keypair::new();
    let authorities = [fixture.authority.pubkey(), second.pubkey(), third.pubkey()];
    for (version, message) in approval_message_versions(&authorities) {
        let expected =
            VersionedTransaction::try_new(message.clone(), &[&fixture.authority, &second, &third])
                .unwrap();
        expected.sanitize().unwrap();
        let message_bytes = message.serialize();
        let encoded_message = BASE64_STANDARD.encode(&message_bytes);
        let mut report = versioned_sign_only(&message, &[&second]);
        // Status lists are not evidence, even for versioned approvals.
        report.absent = vec![second.pubkey().to_string(), address(99).to_string()];
        report.bad_sig = vec![fixture.authority.pubkey().to_string()];
        let mut signed = [false, true, false];
        for (step, (signer, index, format)) in [
            (&fixture.authority, 0, "json"),
            (&third, 2, "json-compact"),
            (&second, 1, "display"),
        ]
        .into_iter()
        .enumerate()
        {
            let input_bytes = serde_json::to_vec(&report).unwrap();
            fs::write(&fixture.input, &input_bytes).unwrap();
            write_keypair_file(signer, &fixture.key).unwrap();
            let path = fixture.output(&format!("{version}-{step}.json"));
            let output = fixture
                .command(&path)
                .args(["--output", format])
                .output()
                .unwrap();
            assert_success(&output);
            assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
            report = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            signed[index] = true;
            assert_eq!(report.message.as_deref(), Some(encoded_message.as_str()));
            assert_eq!(report.blockhash, message.recent_blockhash().to_string());
            assert!(report.bad_sig.is_empty());
            let expected_signers = authorities
                .iter()
                .zip(&expected.signatures)
                .zip(signed)
                .filter(|(_, present)| *present)
                .map(|((key, signature), _)| format!("{key}={signature}"))
                .collect::<Vec<_>>();
            let absent = authorities
                .iter()
                .zip(signed)
                .filter(|(_, present)| !*present)
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>();
            assert_eq!(report.signers, expected_signers, "{version}-{step}");
            assert_eq!(report.absent, absent, "{version}-{step}");
            if format != "display" {
                assert_eq!(
                    serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
                    report
                );
            }
            let parsed = parse_sign_only_reply_string(&serde_json::to_string(&report).unwrap());
            assert_eq!(
                parsed.has_all_signers(),
                signed.into_iter().all(|present| present)
            );
            for (key, signature) in parsed.present_signers {
                assert!(signature.verify(key.as_ref(), &message_bytes));
            }
            let review = String::from_utf8(output.stderr).unwrap();
            assert!(review.contains(&message.hash().to_string()));
            assert!(review.contains("present (verified)"));
        }
    }
}

#[test]
fn cli_preserves_supplied_blockhash_metadata_without_signing_it() {
    let fixture = CliFixture::new();
    let second = Keypair::new();
    for (version, message) in
        approval_message_versions(&[fixture.authority.pubkey(), second.pubkey()])
    {
        for (case, blockhash) in [
            ("malformed", "not-a-hash".to_string()),
            ("empty", String::new()),
            ("different", Hash::new_from_array([12; 32]).to_string()),
        ] {
            let mut expected = versioned_sign_only(&message, &[&fixture.authority, &second]);
            expected.blockhash = blockhash.clone();
            let mut report = versioned_sign_only(&message, &[&second]);
            report.blockhash = blockhash;
            let input_bytes = serde_json::to_vec(&report).unwrap();
            fs::write(&fixture.input, &input_bytes).unwrap();
            let path = fixture.output(&format!("{version}-{case}.json"));
            let output = fixture
                .command(&path)
                .args(["--output", "json"])
                .output()
                .unwrap();
            assert_success(&output);
            let result: CliSignOnlyData =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(result, expected, "{version}-{case}");
            let review = String::from_utf8(output.stderr).unwrap();
            assert!(review.contains(&format!(
                "Blockhash (opaque signed field): {}",
                message.recent_blockhash()
            )));
            assert_eq!(
                serde_json::from_slice::<CliSignOnlyData>(&output.stdout).unwrap(),
                result
            );
            assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
        }
    }
}

#[test]
fn cli_rejects_invalid_versioned_approvals_before_loading_a_wallet() {
    let fixture = CliFixture::new();
    let key = fixture.authority.pubkey();
    let foreign = Keypair::new();
    let legacy_bytes = unsigned(&[key]).message_data();
    fs::write(&fixture.key, "must not load").unwrap();
    for (version, message) in approval_message_versions(&[key]).into_iter().skip(1) {
        let bytes = message.serialize();
        let valid = serde_json::to_value(versioned_sign_only(&message, &[])).unwrap();
        let mut trailing = bytes.clone();
        trailing.push(0);
        let mut unknown_version = bytes.clone();
        unknown_version[0] = 0xff;
        for (name, field, value, expected_error) in [
            (
                "trailing",
                "message",
                serde_json::json!(BASE64_STANDARD.encode(trailing)),
                "invalid serialized message",
            ),
            (
                "truncated",
                "message",
                serde_json::json!(BASE64_STANDARD.encode(&bytes[..10])),
                "invalid serialized message",
            ),
            (
                "unknown-version",
                "message",
                serde_json::json!(BASE64_STANDARD.encode(unknown_version)),
                "invalid serialized message",
            ),
            (
                "legacy-signature",
                "signers",
                serde_json::json!([format!(
                    "{key}={}",
                    fixture.authority.sign_message(&legacy_bytes)
                )]),
                "invalid existing approval signature",
            ),
            (
                "foreign-signer",
                "signers",
                serde_json::json!([format!(
                    "{}={}",
                    foreign.pubkey(),
                    foreign.sign_message(&bytes)
                )]),
                "invalid existing approval signature",
            ),
            (
                "zero-signature",
                "signers",
                serde_json::json!([format!("{key}={}", Signature::default())]),
                "invalid existing approval signature",
            ),
            (
                "shadowed-invalid-signature",
                "signers",
                serde_json::json!([
                    format!("{key}={}", Signature::from([5; 64])),
                    format!("{key}={}", fixture.authority.sign_message(&bytes)),
                ]),
                "invalid existing approval signature",
            ),
        ] {
            let mut report = valid.clone();
            report[field] = value;
            let input_bytes = serde_json::to_vec(&report).unwrap();
            fs::write(&fixture.input, &input_bytes).unwrap();
            let path = fixture.output(&format!("{version}-{name}.json"));
            let output = fixture.command(&path).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(1),
                "{version}-{name}: {output:?}"
            );
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains(expected_error), "{version}-{name}: {error}");
            assert!(
                !error.contains("failed to load approval authority"),
                "{error}"
            );
            assert!(!error.contains("panicked"), "{error}");
            assert!(output.stdout.is_empty());
            assert!(!path.exists());
            assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
        }
    }
}

#[test]
fn cli_rejects_v0_executor_accounts_that_require_lookup_resolution() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "must not load").unwrap();
    let (_, message) = approval_message_versions(&[fixture.authority.pubkey()])[2].clone();
    let VersionedMessage::V0(message) = message else {
        unreachable!()
    };
    // Check the nonce index and a later account, not just the first index used by the review.
    for (name, position) in [
        ("nonce", 0),
        (
            "later-account",
            message.instructions[0].accounts.len().saturating_sub(1),
        ),
    ] {
        let mut message = message.clone();
        message.instructions[0].accounts[position] =
            u8::try_from(message.account_keys.len()).unwrap();
        let message = VersionedMessage::V0(message);
        message.sanitize().unwrap();
        let report = versioned_sign_only(&message, &[]);
        let input_bytes = serde_json::to_vec(&report).unwrap();
        fs::write(&fixture.input, &input_bytes).unwrap();
        let path = fixture.output(&format!("lookup-{name}.json"));
        let output = fixture.command(&path).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("executor accounts must use static account keys"),
            "{error}"
        );
        assert!(
            !error.contains("failed to load approval authority"),
            "{error}"
        );
        assert!(!error.contains("panicked"), "{error}");
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
        assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
    }
}

#[test]
fn cli_rejects_v1_config_bits_that_cannot_be_preserved() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "must not load").unwrap();
    let (_, message) = approval_message_versions(&[fixture.authority.pubkey()])[3].clone();
    for (name, mask) in [("unknown-bit", 0x3f), ("partial-priority-fee", 0x1d)] {
        let mut bytes = message.serialize();
        // The config mask follows the version prefix and three-byte message header.
        bytes[4] = mask;
        let mut report = versioned_sign_only(&message, &[]);
        report.message = Some(BASE64_STANDARD.encode(bytes));
        let input_bytes = serde_json::to_vec(&report).unwrap();
        fs::write(&fixture.input, &input_bytes).unwrap();
        let path = fixture.output(&format!("v1-{name}.json"));
        let output = fixture.command(&path).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("invalid serialized message"), "{error}");
        assert!(
            !error.contains("failed to load approval authority"),
            "{error}"
        );
        assert!(!error.contains("panicked"), "{error}");
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
        assert_eq!(fs::read(&fixture.input).unwrap(), input_bytes);
    }
}

#[test]
fn cli_review_shows_the_message_hash_and_unverified_execution_details() {
    let fixture = CliFixture::new();
    let mut hashes = Vec::new();
    for lamports in [1, 2] {
        let instruction = transfer(&pda(&fixture.authority.pubkey()), &address(3), lamports);
        let data = format!("data={:02x?}", instruction.data);
        let transaction = envelope_with_instructions(&[fixture.authority.pubkey()], &[instruction]);
        fs::write(
            &fixture.input,
            serde_json::to_vec(&sign_only(&transaction)).unwrap(),
        )
        .unwrap();
        let path = fixture.output(&format!("signed-{lamports}.json"));
        let output = fixture.command(&path).output().unwrap();
        assert_success(&output);
        let review = String::from_utf8(output.stderr).unwrap();
        let hash = format!(
            "Message hash (BLAKE3): {}",
            Message::hash_raw_message(&transaction.message_data())
        );
        for text in [
            "No separate Submit relay is constructed or signed",
            "nothing is submitted to the network",
            "SPL nonce account",
            "SPL nonce value",
            "data shown as hex, not decoded",
            "execution validity",
            "cluster identity",
            &data,
            "Blockhash (opaque signed field):",
            &fixture.authority.pubkey().to_string(),
            &pda(&fixture.authority.pubkey()).to_string(),
            &address(2).to_string(),
            &address(3).to_string(),
            &hash,
        ] {
            assert!(review.contains(text), "missing review field {text}");
        }
        // A repeated signing reviews the same message hash, despite the populated signature.
        fs::copy(&path, &fixture.input).unwrap();
        let repeated = fixture.output(&format!("repeated-{lamports}.json"));
        let output = fixture.command(&repeated).output().unwrap();
        assert_success(&output);
        assert!(String::from_utf8(output.stderr).unwrap().contains(&hash));
        assert_eq!(read_transaction(&path), read_transaction(&repeated));
        hashes.push(hash);
    }
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn cli_rejects_invalid_json_and_accepts_large_valid_json() {
    let fixture = CliFixture::new();
    let original = fs::read(&fixture.input).unwrap();
    let path = fixture.output("signed.json");
    let mut trailing = original.clone();
    trailing.extend_from_slice(b" {}");
    for bytes in [b"not JSON".to_vec(), b"{}".to_vec(), vec![255], trailing] {
        fs::write(&fixture.input, &bytes).unwrap();
        let output = fixture.command(&path).output().unwrap();
        assert!(!output.status.success());
        assert!(!path.exists());
        assert_eq!(fs::read(&fixture.input).unwrap(), bytes);
    }
    // Valid JSON remains accepted beyond the former 1 MiB input limit.
    let mut padded = original;
    padded.resize(1_048_577, b' ');
    fs::write(&fixture.input, &padded).unwrap();
    assert_success(&fixture.command(&path).output().unwrap());
    assert_eq!(fs::read(&fixture.input).unwrap(), padded);
}

#[test]
fn cli_never_overwrites_files_or_directories() {
    let fixture = CliFixture::new();
    let original = fs::read(&fixture.input).unwrap();
    let existing = fixture.output("existing.json");
    fs::write(&existing, "keep").unwrap();
    for path in [&existing, &fixture.input, fixture.directory.path()] {
        let output = fixture.command(path).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(&format!("failed to create {}", path.display())),
            "{error}"
        );
        assert!(output.stdout.is_empty());
    }
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
    assert_eq!(fs::read(&fixture.input).unwrap(), original);
}

#[test]
fn cli_does_not_create_an_output_file_on_wallet_load_failure() {
    let fixture = CliFixture::new();
    fs::write(&fixture.key, "not a keypair").unwrap();
    let path = fixture.output("failed.json");
    let output = fixture.command(&path).output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("failed to load approval authority"),
        "{error}"
    );
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
}

#[test]
fn cli_does_not_print_a_sign_only_report_when_the_output_file_cannot_be_created() {
    let fixture = CliFixture::new();
    let original = fs::read(&fixture.input).unwrap();
    let path = fixture.output("missing-directory/signed.json");
    let output = fixture
        .command(&path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains(&format!("failed to create {}", path.display())),
        "{error}"
    );
    assert!(output.stdout.is_empty());
    assert!(!path.exists());
    assert_eq!(fs::read(&fixture.input).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn cli_never_follows_live_or_dangling_output_symlinks() {
    let fixture = CliFixture::new();
    for present in [false, true] {
        let target = fixture.output(&format!("target-{present}"));
        let link = fixture.output(&format!("link-{present}"));
        if present {
            fs::write(&target, "keep").unwrap();
        }
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let output = fixture.command(&link).output().unwrap();
        assert!(!output.status.success());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(target.exists(), present);
        if present {
            assert_eq!(fs::read_to_string(target).unwrap(), "keep");
        }
    }
}
