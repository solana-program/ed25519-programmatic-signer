use {
    assert_cmd::Command,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde_json::Value,
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair},
    solana_message::{VersionedMessage, legacy::Message},
    solana_signer::Signer as _,
    solana_system_interface::instruction as system_instruction,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    std::{fs, path::Path},
    tempfile::TempDir,
};

#[test]
fn offline_transaction_flow_uses_sign_only_json_as_input() {
    let fixture = Fixture::new();

    psigner()
        .args(["address", &fixture.authority.pubkey().to_string()])
        .assert()
        .success()
        .stdout(format!("{}\n", fixture.programmatic_signer));

    psigner()
        .args([
            "transaction",
            "create",
            "--from-sign-only",
            path_str(&fixture.sign_only_path),
            "--nonce",
            &fixture.nonce_account.to_string(),
            "--authority",
            &fixture.authority.pubkey().to_string(),
            "--nonce-value",
            &fixture.nonce_value.to_string(),
            "--genesis-hash",
            &fixture.genesis_hash.to_string(),
            "--outfile",
            path_str(&fixture.transaction_path),
        ])
        .assert()
        .success();

    let inspect = psigner_json([
        "--output",
        "json-compact",
        "transaction",
        "inspect",
        path_str(&fixture.transaction_path),
    ]);
    assert_eq!(
        inspect["nonceAccount"],
        Value::String(fixture.nonce_account.to_string())
    );
    assert_eq!(
        inspect["genesisHash"],
        Value::String(fixture.genesis_hash.to_string())
    );
    assert_eq!(inspect["innerInstructions"].as_array().unwrap().len(), 1);

    let verify_unsigned = psigner_json([
        "--output",
        "json-compact",
        "transaction",
        "verify",
        path_str(&fixture.transaction_path),
        "--nonce-value",
        &fixture.nonce_value.to_string(),
        "--nonce-authority",
        &fixture.programmatic_signer.to_string(),
        "--genesis-hash",
        &fixture.genesis_hash.to_string(),
    ]);
    assert_eq!(verify_unsigned["fullySigned"], Value::Bool(false));

    psigner()
        .args([
            "transaction",
            "sign",
            path_str(&fixture.transaction_path),
            "--keypair",
            path_str(&fixture.authority_path),
            "--outfile",
            path_str(&fixture.signed_transaction_path),
        ])
        .assert()
        .success();

    let verify_signed = psigner_json([
        "--output",
        "json-compact",
        "transaction",
        "verify",
        path_str(&fixture.signed_transaction_path),
        "--nonce-value",
        &fixture.nonce_value.to_string(),
        "--nonce-authority",
        &fixture.programmatic_signer.to_string(),
        "--genesis-hash",
        &fixture.genesis_hash.to_string(),
    ]);
    assert_eq!(verify_signed["fullySigned"], Value::Bool(true));

    psigner()
        .args([
            "transaction",
            "merge",
            path_str(&fixture.transaction_path),
            path_str(&fixture.signed_transaction_path),
            "--outfile",
            path_str(&fixture.merged_transaction_path),
        ])
        .assert()
        .success();

    let verify_merged = psigner_json([
        "--output",
        "json-compact",
        "transaction",
        "verify",
        path_str(&fixture.merged_transaction_path),
        "--nonce-value",
        &fixture.nonce_value.to_string(),
        "--nonce-authority",
        &fixture.programmatic_signer.to_string(),
        "--genesis-hash",
        &fixture.genesis_hash.to_string(),
    ]);
    assert_eq!(verify_merged["fullySigned"], Value::Bool(true));

    psigner()
        .args([
            "transaction",
            "submit",
            path_str(&fixture.signed_transaction_path),
            "--fee-payer",
            path_str(&fixture.fee_payer_path),
            "--blockhash",
            &fixture.submit_blockhash.to_string(),
            "--no-send",
            "--outfile",
            path_str(&fixture.submit_transaction_path),
        ])
        .assert()
        .success();
    let submit_transaction = fs::read_to_string(&fixture.submit_transaction_path).unwrap();
    assert!(submit_transaction.len() > 100);
    STANDARD.decode(submit_transaction.trim()).unwrap();
}

#[test]
fn nonce_advance_from_transaction_builds_cancellation_transaction() {
    let fixture = Fixture::new();

    psigner()
        .args([
            "transaction",
            "create",
            "--from-sign-only",
            path_str(&fixture.sign_only_path),
            "--nonce",
            &fixture.nonce_account.to_string(),
            "--authority",
            &fixture.authority.pubkey().to_string(),
            "--nonce-value",
            &fixture.nonce_value.to_string(),
            "--genesis-hash",
            &fixture.genesis_hash.to_string(),
            "--outfile",
            path_str(&fixture.transaction_path),
        ])
        .assert()
        .success();

    psigner()
        .args([
            "nonce",
            "advance",
            "--from-transaction",
            path_str(&fixture.transaction_path),
            "--authority",
            &fixture.authority.pubkey().to_string(),
            "--outfile",
            path_str(&fixture.advance_transaction_path),
        ])
        .assert()
        .success();

    let inspect = psigner_json([
        "--output",
        "json-compact",
        "transaction",
        "inspect",
        path_str(&fixture.advance_transaction_path),
    ]);
    assert_eq!(
        inspect["nonceAccount"],
        Value::String(fixture.nonce_account.to_string())
    );
    assert_eq!(inspect["innerInstructions"].as_array().unwrap().len(), 0);
}

#[test]
fn command_help_documents_each_primary_command() {
    psigner()
        .args(["transaction", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Wrap Solana CLI or SPL Token CLI sign-only JSON",
        ))
        .stdout(predicates::str::contains(
            "Build and send the outer Submit transaction",
        ));

    psigner()
        .args(["transaction", "simulate", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("inner"))
        .stdout(predicates::str::contains("relay"));

    psigner()
        .args(["transaction", "sign", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("KEYPAIR_OR_URL"));

    psigner()
        .args(["transaction", "submit", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("KEYPAIR_OR_URL"));

    psigner()
        .args(["nonce", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Create and initialize one SPL Nonce account",
        ))
        .stdout(predicates::str::contains(
            "Build a nonce-consuming cancellation transaction",
        ));
}

#[test]
fn command_parser_rejects_ambiguous_modes() {
    let fixture = Fixture::new();

    psigner()
        .args([
            "transaction",
            "sign",
            "--keypair",
            path_str(&fixture.authority_path),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("TRANSACTION"));

    psigner()
        .args(["transaction", "merge", path_str(&fixture.transaction_path)])
        .assert()
        .failure()
        .stderr(predicates::str::contains("2 values required"));

    psigner()
        .args([
            "transaction",
            "verify",
            path_str(&fixture.transaction_path),
            "--fetch-nonce",
            "--nonce-value",
            &fixture.nonce_value.to_string(),
            "--nonce-authority",
            &fixture.programmatic_signer.to_string(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));

    psigner()
        .args([
            "transaction",
            "simulate",
            "relay",
            path_str(&fixture.transaction_path),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--fee-payer"));

    psigner()
        .args([
            "transaction",
            "submit",
            path_str(&fixture.transaction_path),
            "--fee-payer",
            path_str(&fixture.fee_payer_path),
            "--outfile",
            path_str(&fixture.submit_transaction_path),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--no-send"));

    psigner()
        .args([
            "transaction",
            "sign",
            path_str(&fixture.transaction_path),
            "--keypair",
            "https://example.com/keypair",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unsupported signer URL scheme"));
}

struct Fixture {
    _temp_dir: TempDir,
    authority: Keypair,
    nonce_account: Address,
    nonce_value: Hash,
    genesis_hash: Hash,
    submit_blockhash: Hash,
    programmatic_signer: Address,
    sign_only_path: std::path::PathBuf,
    authority_path: std::path::PathBuf,
    fee_payer_path: std::path::PathBuf,
    transaction_path: std::path::PathBuf,
    signed_transaction_path: std::path::PathBuf,
    merged_transaction_path: std::path::PathBuf,
    advance_transaction_path: std::path::PathBuf,
    submit_transaction_path: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let authority = Keypair::new();
        let fee_payer = Keypair::new();
        let recipient = Keypair::new();
        let nonce_account = Keypair::new().pubkey();
        let nonce_value = Hash::new_from_array([1; 32]);
        let genesis_hash = Hash::new_from_array([2; 32]);
        let submit_blockhash = Hash::new_from_array([3; 32]);
        let programmatic_signer = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_interface::id(),
            &authority.pubkey(),
        );
        let mut message = Message::new(
            &[system_instruction::transfer(
                &programmatic_signer,
                &recipient.pubkey(),
                42,
            )],
            Some(&programmatic_signer),
        );
        message.recent_blockhash = nonce_value;
        let sign_only = serde_json::json!({
            "blockhash": nonce_value.to_string(),
            "message": STANDARD.encode(VersionedMessage::Legacy(message).serialize()),
            "absent": [programmatic_signer.to_string()],
        });

        let sign_only_path = temp_dir.path().join("inner.json");
        let authority_path = temp_dir.path().join("authority.json");
        let fee_payer_path = temp_dir.path().join("fee-payer.json");
        let transaction_path = temp_dir.path().join("tx.psigner.json");
        let signed_transaction_path = temp_dir.path().join("tx.signed.psigner.json");
        let merged_transaction_path = temp_dir.path().join("tx.merged.psigner.json");
        let advance_transaction_path = temp_dir.path().join("advance.psigner.json");
        let submit_transaction_path = temp_dir.path().join("submit.json");

        fs::write(&sign_only_path, serde_json::to_string(&sign_only).unwrap()).unwrap();
        write_keypair_file(&authority_path, &authority);
        write_keypair_file(&fee_payer_path, &fee_payer);

        Self {
            _temp_dir: temp_dir,
            authority,
            nonce_account,
            nonce_value,
            genesis_hash,
            submit_blockhash,
            programmatic_signer,
            sign_only_path,
            authority_path,
            fee_payer_path,
            transaction_path,
            signed_transaction_path,
            merged_transaction_path,
            advance_transaction_path,
            submit_transaction_path,
        }
    }
}

fn psigner() -> Command {
    Command::cargo_bin("psigner").unwrap()
}

fn psigner_json<const N: usize>(args: [&str; N]) -> Value {
    let output = psigner().args(args).assert().success().get_output().clone();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn write_keypair_file(path: &Path, keypair: &Keypair) {
    let mut file = fs::File::create(path).unwrap();
    write_keypair(keypair, &mut file).unwrap();
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}
