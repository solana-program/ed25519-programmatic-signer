//! Signer precedence and RPC configuration through the CLI, without network requests.

use {
    solana_cli_config::Config as SolanaConfig,
    solana_keypair::{Keypair, write_keypair_file},
    solana_signer::Signer,
    std::{fs, process::Command},
};

#[test]
fn nonce_fee_payer_uses_role_override_then_global_keypair_then_config() {
    let directory = tempfile::tempdir().unwrap();
    let keypair = Keypair::new();
    let key = directory.path().join("keypair.json");
    let invalid_key = directory.path().join("invalid-keypair.json");
    let config = directory.path().join("config.yml");
    write_keypair_file(&keypair, &key).unwrap();
    fs::write(&invalid_key, "must not load this keypair").unwrap();
    let valid = key.to_str().unwrap();
    let invalid = invalid_key.to_str().unwrap();
    for (configured_key, overrides) in [
        (valid, vec![]),
        (invalid, vec!["--keypair", valid]),
        (invalid, vec!["--keypair", invalid, "--fee-payer", valid]),
    ] {
        SolanaConfig {
            json_rpc_url: "http://127.0.0.1:1".to_string(),
            commitment: "confirmed".to_string(),
            keypair_path: configured_key.to_string(),
            ..SolanaConfig::default()
        }
        .save(config.to_str().unwrap())
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
            .arg("--config")
            .arg(&config)
            .args(&overrides)
            .args(["nonce", "create", "--nonce-authority"])
            .arg(keypair.pubkey().to_string())
            .arg("--nonce-keypair")
            .arg(&key)
            .output()
            .unwrap();
        // The selected payer matches the nonce key, triggering this guard before rent/blockhash
        // queries. Any incorrect fallback instead fails to load the intentionally invalid key.
        assert_eq!(output.status.code(), Some(1), "{overrides:?}: {output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("fee payer and nonce account must use different keypairs"),
            "{overrides:?}: {error}"
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn commands_reject_invalid_rpc_settings_during_client_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.yml");
    let key = directory.path().join("invalid-keypair.json");
    let input = directory.path().join("invalid-artifact.json");
    let outfile = directory.path().join("signed.json");
    fs::write(&key, "not a keypair").unwrap();
    fs::write(&input, "not an artifact").unwrap();
    for (url, commitment, expected) in [
        ("not an RPC URL", "confirmed", "invalid RPC URL"),
        (
            "http://127.0.0.1:1",
            "not a commitment",
            "invalid commitment",
        ),
    ] {
        SolanaConfig {
            json_rpc_url: url.to_string(),
            commitment: commitment.to_string(),
            keypair_path: key.to_str().unwrap().to_string(),
            ..SolanaConfig::default()
        }
        .save(config.to_str().unwrap())
        .unwrap();
        for args in [
            vec![
                "nonce",
                "create",
                "--nonce-authority",
                "11111111111111111111111111111111",
            ],
            vec![
                "tx",
                "sign",
                input.to_str().unwrap(),
                "--outfile",
                outfile.to_str().unwrap(),
            ],
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
                .arg("--config")
                .arg(&config)
                .args(args)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            let error = String::from_utf8(output.stderr).unwrap();
            // Client initialization must fail before reading the invalid wallet or artifact.
            assert!(error.contains(expected), "{error}");
            assert!(output.stdout.is_empty());
            assert!(!outfile.exists());
        }
    }
}
