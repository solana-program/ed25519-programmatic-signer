use {
    crate::common::helpers::{TestEnv, run_psigner, run_psigner_with_input},
    solana_keypair::{Keypair, write_keypair_file},
    solana_signer::Signer,
    spl_programmatic_signer_cli::{NonceAdvanceOutput, NonceCreateOutput, NonceShowOutput},
    tempfile::NamedTempFile,
};

fn create_nonce(env: &TestEnv, authority_arg: &str, authority: &str) -> NonceCreateOutput {
    let create = run_psigner(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "create",
        authority_arg,
        authority,
    ]);
    serde_json::from_slice(&create.stdout).unwrap()
}

/// Advance the nonce, optionally with an explicit nonce authority signer, and check the stored
/// nonce moved on.
fn advance_nonce(env: &TestEnv, create: &NonceCreateOutput, nonce_authority: Option<&str>) {
    let mut args = vec![
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "advance",
        &create.nonce_account,
    ];
    if let Some(nonce_authority) = nonce_authority {
        args.extend(["--nonce-authority", nonce_authority]);
    }
    let advance = run_psigner(&args);
    let advance: NonceAdvanceOutput = serde_json::from_slice(&advance.stdout).unwrap();
    assert_eq!(advance.nonce_account, create.nonce_account);
    assert_eq!(advance.previous_nonce, create.nonce);
    assert_ne!(advance.nonce, create.nonce);
    assert!(!advance.signature.is_empty());

    let show = run_psigner(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "show",
        &create.nonce_account,
    ]);
    let show: NonceShowOutput = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(show.nonce, advance.nonce);
    assert_eq!(show.authority, create.authority);
}

pub async fn advances_nonce_with_plain_key_authority(env: &TestEnv) {
    // The configured keypair is both the nonce authority and the fee payer.
    let create = create_nonce(env, "--nonce-authority", &env.payer_address);
    advance_nonce(env, &create, None);

    // A separate nonce authority signs alongside the fee payer.
    let authority = Keypair::new();
    let authority_file = NamedTempFile::new().unwrap();
    write_keypair_file(&authority, &authority_file).unwrap();
    let create = create_nonce(env, "--nonce-authority", &authority.pubkey().to_string());
    advance_nonce(env, &create, Some(authority_file.path().to_str().unwrap()));
}

pub async fn advances_nonce_with_cold_authority(env: &TestEnv) {
    // The configured keypair is both the cold authority and the fee payer.
    let create = create_nonce(env, "--cold-authority", &env.payer_address);
    advance_nonce(env, &create, None);

    // An unfunded cold authority signs only the authorization message.
    let cold_authority = Keypair::new();
    let cold_authority_file = NamedTempFile::new().unwrap();
    write_keypair_file(&cold_authority, &cold_authority_file).unwrap();
    let create = create_nonce(
        env,
        "--cold-authority",
        &cold_authority.pubkey().to_string(),
    );
    advance_nonce(
        env,
        &create,
        Some(cold_authority_file.path().to_str().unwrap()),
    );
}

pub async fn rejects_unrelated_nonce_authority(env: &TestEnv) {
    let authority = Keypair::new().pubkey().to_string();
    let create = create_nonce(env, "--nonce-authority", &authority);
    let advance = run_psigner_with_input(
        &[
            "-C",
            &env.config_file_path,
            "nonce",
            "advance",
            &create.nonce_account,
        ],
        "",
    );
    assert!(!advance.status.success());
    let stderr = String::from_utf8_lossy(&advance.stderr);
    assert!(
        stderr.contains(&format!(
            "nonce account {} has authority {authority}, but the authority signer is {}",
            create.nonce_account, env.payer_address
        )),
        "{stderr}"
    );
}
