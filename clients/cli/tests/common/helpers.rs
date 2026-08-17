use {
    solana_cli_config::Config as SolanaConfig,
    solana_commitment_config::CommitmentConfig,
    solana_keypair::write_keypair_file,
    solana_signer::Signer,
    solana_test_validator::{TestValidator, TestValidatorGenesis},
    std::process::{Command, Output},
    tempfile::NamedTempFile,
};

pub struct TestEnv {
    pub payer_address: String,
    pub config_file_path: String,
    _validator: TestValidator,
    _payer_file: NamedTempFile,
    _config_file: NamedTempFile,
}

pub async fn setup_test_env() -> TestEnv {
    let mut genesis = TestValidatorGenesis::default_for_tests();
    genesis.add_program("spl_nonce_program", spl_nonce_interface::id());
    let (validator, payer) = genesis.start_async().await;

    let payer_address = payer.pubkey().to_string();
    let payer_file = NamedTempFile::new().unwrap();
    write_keypair_file(&payer, &payer_file).unwrap();

    let config_file = NamedTempFile::new().unwrap();
    let config_file_path = config_file.path().to_str().unwrap().to_string();

    SolanaConfig {
        json_rpc_url: validator.rpc_url(),
        websocket_url: validator.rpc_pubsub_url(),
        keypair_path: payer_file.path().to_str().unwrap().to_string(),
        commitment: CommitmentConfig::confirmed().commitment.to_string(),
        ..SolanaConfig::default()
    }
    .save(&config_file_path)
    .unwrap();

    TestEnv {
        payer_address,
        config_file_path,
        _validator: validator,
        _payer_file: payer_file,
        _config_file: config_file,
    }
}

pub fn run_psigner(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_psigner"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "psigner failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}
