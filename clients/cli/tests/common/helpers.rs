use {
    solana_cli_config::Config as SolanaConfig,
    solana_commitment_config::CommitmentConfig,
    solana_keypair::write_keypair_file,
    solana_signer::Signer,
    solana_test_validator::{TestValidator, TestValidatorGenesis},
    spl_nonce_interface::state::Nonce,
    std::{
        io::Write,
        process::{Command, Output, Stdio},
    },
    tempfile::NamedTempFile,
};

pub struct TestEnv {
    pub payer_address: String,
    pub config_file_path: String,
    pub nonce_rent_lamports: u64,
    _validator: TestValidator,
    _payer_file: NamedTempFile,
    _config_file: NamedTempFile,
}

pub async fn setup_test_env() -> TestEnv {
    let mut genesis = TestValidatorGenesis::default_for_tests();
    genesis.add_program("spl_nonce_program", spl_nonce_interface::id());
    let (validator, payer) = genesis.start_async().await;
    let nonce_rent_lamports = validator
        .get_async_rpc_client()
        .get_minimum_balance_for_rent_exemption(Nonce::LEN)
        .await
        .unwrap();

    let payer_address = payer.pubkey().to_string();
    let payer_file = NamedTempFile::new().unwrap();
    write_keypair_file(&payer, &payer_file).unwrap();

    let config_file = NamedTempFile::new().unwrap();
    let config_file_path = config_file.path().to_str().unwrap().to_string();

    SolanaConfig {
        json_rpc_url: validator.rpc_url(),
        websocket_url: validator.rpc_pubsub_url(),
        keypair_path: payer_file.path().to_str().unwrap().to_string(),
        commitment: CommitmentConfig::processed().commitment.to_string(),
        ..SolanaConfig::default()
    }
    .save(&config_file_path)
    .unwrap();

    TestEnv {
        payer_address,
        config_file_path,
        nonce_rent_lamports,
        _validator: validator,
        _payer_file: payer_file,
        _config_file: config_file,
    }
}

pub fn run_psigner(args: &[&str]) -> Output {
    let output = run_psigner_with_input(args, "");
    assert!(
        output.status.success(),
        "spl-programmatic-signer-cli failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

/// Run the CLI with preset input such as `"y\n"` to answer a confirmation prompt.
/// Input is written as soon as the process starts and buffered until the CLI reads
/// it. After writing, stdin is closed and this helper waits for the process to exit.
pub fn run_psigner_with_input(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// Assert that a CLI failure emits and outputs the expected error.
pub fn assert_failure(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(stderr.contains(expected));
}
