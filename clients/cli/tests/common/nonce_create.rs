use {
    crate::common::helpers::{TestEnv, run_signer},
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_signature::Signature,
    solana_signer::Signer,
    spl_programmatic_signer_cli::output::{NonceCreateOutput, NonceShowOutput},
    tempfile::NamedTempFile,
};

pub async fn creates_and_shows_nonce_account(env: &TestEnv) {
    let nonce_keypair = Keypair::new();
    let nonce_keypair_file = NamedTempFile::new().unwrap();
    write_keypair_file(&nonce_keypair, &nonce_keypair_file).unwrap();
    let nonce_account = nonce_keypair.pubkey().to_string();

    let create = run_signer(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "create",
        "--nonce-authority",
        &env.payer_address,
        "--nonce-keypair",
        nonce_keypair_file.path().to_str().unwrap(),
    ]);
    let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();
    assert_eq!(create.nonce_account, nonce_account);
    assert_eq!(create.authority, env.payer_address);
    assert_eq!(create.lamports, env.nonce_rent_lamports);
    assert_ne!(
        create.signature.parse::<Signature>().unwrap(),
        Signature::default()
    );
    assert_ne!(create.nonce.parse::<Hash>().unwrap(), Hash::default());
    assert!(create.lamports > 0);

    let show = run_signer(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "show",
        &nonce_account,
    ]);
    let show: NonceShowOutput = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(show.nonce_account, create.nonce_account);
    assert_eq!(show.authority, create.authority);
    assert_eq!(show.nonce, create.nonce);
    assert_eq!(show.lamports, create.lamports);
    assert_eq!(show.owner, spl_nonce_interface::id().to_string());
}

pub async fn creates_nonce_account_with_generated_keypair(env: &TestEnv) {
    let authority = Keypair::new().pubkey();
    let programmatic_signer = spl_ed25519_signer_client::ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_client::id(),
        &authority,
    );
    for (authority_arg, expected_authority) in [
        ("--nonce-authority", authority),
        ("--cold-authority", programmatic_signer),
    ] {
        let create = run_signer(&[
            "-C",
            &env.config_file_path,
            "--output",
            "json-compact",
            "nonce",
            "create",
            authority_arg,
            &authority.to_string(),
        ]);
        let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();
        assert_ne!(create.nonce_account, env.payer_address);
        assert_eq!(create.authority, expected_authority.to_string());
        assert_eq!(create.lamports, env.nonce_rent_lamports);
        assert_ne!(
            create.signature.parse::<Signature>().unwrap(),
            Signature::default()
        );
        assert_ne!(create.nonce.parse::<Hash>().unwrap(), Hash::default());
        assert!(create.lamports > 0);

        let show = run_signer(&[
            "-C",
            &env.config_file_path,
            "--output",
            "json-compact",
            "nonce",
            "show",
            &create.nonce_account,
        ]);
        let show: NonceShowOutput = serde_json::from_slice(&show.stdout).unwrap();
        assert_eq!(show.nonce_account, create.nonce_account);
        assert_eq!(show.authority, create.authority);
        assert_eq!(show.nonce, create.nonce);
        assert_eq!(show.lamports, create.lamports);
        assert_eq!(show.owner, spl_nonce_interface::id().to_string());
    }
}

pub async fn creates_nonce_account_with_cold_authority(env: &TestEnv) {
    let cold_authority = Keypair::new().pubkey();
    let nonce_keypair = Keypair::new();
    let nonce_keypair_file = NamedTempFile::new().unwrap();
    write_keypair_file(&nonce_keypair, &nonce_keypair_file).unwrap();

    let create = run_signer(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "create",
        "--cold-authority",
        &cold_authority.to_string(),
        "--nonce-keypair",
        nonce_keypair_file.path().to_str().unwrap(),
    ]);
    let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();

    let programmatic_signer = spl_ed25519_signer_client::ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_client::id(),
        &cold_authority,
    );
    assert_eq!(create.nonce_account, nonce_keypair.pubkey().to_string());
    assert_eq!(create.authority, programmatic_signer.to_string());
    assert_eq!(create.lamports, env.nonce_rent_lamports);
}
