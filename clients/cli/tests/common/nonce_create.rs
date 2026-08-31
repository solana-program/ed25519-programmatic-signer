use {
    crate::common::helpers::{TestEnv, run_psigner},
    serde::Deserialize,
    solana_keypair::{Keypair, write_keypair_file},
    solana_signer::Signer,
    tempfile::NamedTempFile,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NonceCreateOutput {
    signature: String,
    nonce_account: String,
    authority: String,
    nonce: String,
    lamports: u64,
    rent_lamports: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NonceShowOutput {
    nonce_account: String,
    authority: String,
    nonce: String,
    lamports: u64,
    owner: String,
}

pub async fn creates_and_shows_nonce_account(env: &TestEnv) {
    let nonce_keypair = Keypair::new();
    let nonce_keypair_file = NamedTempFile::new().unwrap();
    write_keypair_file(&nonce_keypair, &nonce_keypair_file).unwrap();
    let nonce_account = nonce_keypair.pubkey().to_string();

    let create = run_psigner(&[
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
    assert_eq!(create.lamports, create.rent_lamports);
    assert!(!create.signature.is_empty());

    let show = run_psigner(&[
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

pub async fn creates_nonce_account_with_cold_authority(env: &TestEnv) {
    let cold_authority = Keypair::new().pubkey();
    let nonce_keypair = Keypair::new();
    let nonce_keypair_file = NamedTempFile::new().unwrap();
    write_keypair_file(&nonce_keypair, &nonce_keypair_file).unwrap();

    let create = run_psigner(&[
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
    assert_eq!(create.lamports, create.rent_lamports);
}
