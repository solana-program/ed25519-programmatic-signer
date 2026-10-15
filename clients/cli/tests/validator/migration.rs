//! Helpers shared by the `migrate` command tests.

use {
    crate::common::{
        execute::programmatic_signer,
        helpers::{TestEnv, run_psigner, run_psigner_with_input},
    },
    base64::{Engine, prelude::BASE64_STANDARD},
    serde::{Deserialize, de::DeserializeOwned},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::{VersionedMessage, v1},
    solana_signer::Signer,
    solana_transaction::Transaction,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_cli::{Authorization, NonceCreateOutput, RequiredSigners},
    tempfile::NamedTempFile,
};

/// The authorization an authority gives for its programmatic signer.
pub fn authorization(authority: &Address) -> Authorization {
    Authorization {
        programmatic_signer: programmatic_signer(authority).to_string(),
        authority: Some(authority.to_string()),
    }
}

pub fn decode_execution_message(execution_message: &str) -> v1::Message {
    let bytes = BASE64_STANDARD.decode(execution_message).unwrap();
    let VersionedMessage::V1(message) = wincode::deserialize_exact(&bytes).unwrap() else {
        panic!("expected a v1 execution message");
    };
    message
}

/// Forwarded signers are compared in any order.
pub fn assert_signers(
    signers: &RequiredSigners,
    authorizations: &[Authorization],
    forwarded_signers: &[Address],
) {
    assert_eq!(signers.authorizations, authorizations);
    let mut actual = signers.forwarded_signers.clone();
    actual.sort();
    let mut expected = forwarded_signers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(actual, expected);
}

#[derive(Deserialize)]
struct SignOutput {
    authorization_message: String,
    signatures: Vec<SignatureEntry>,
}

#[derive(Deserialize)]
struct SignatureEntry {
    address: String,
    signature: String,
}

/// Send a transaction paid for by the test payer.
pub async fn send(env: &TestEnv, instructions: &[Instruction], signers: &[&Keypair]) {
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&transaction)
        .await
        .unwrap();
}

/// An SPL nonce account whose authority is `authority`'s programmatic signer.
pub fn create_nonce(env: &TestEnv, authority: &Address) -> Address {
    let create = run_psigner(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json-compact",
        "nonce",
        "create",
        "--cold-authority",
        &authority.to_string(),
    ]);
    let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();
    create.nonce_account.parse().unwrap()
}

async fn current_nonce(env: &TestEnv, nonce_account: &Address) -> Hash {
    let data = env.rpc.get_account_data(nonce_account).await.unwrap();
    Nonce::view(&data).unwrap().nonce
}

fn keypair_file(keypair: &Keypair) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    write_keypair_file(keypair, &file).unwrap();
    file
}

/// Run `migrate <kind> <account> --new-owner <new_owner> [extra]` and parse its JSON output.
pub fn migrate<T: DeserializeOwned>(
    env: &TestEnv,
    kind: &str,
    account: &Address,
    new_owner: &Address,
    extra: &[&str],
) -> T {
    let account = account.to_string();
    let new_owner = new_owner.to_string();
    let mut args = vec![kind, &account, "--new-owner", &new_owner];
    args.extend_from_slice(extra);
    migrate_with_args(env, &args)
}

/// Run `migrate <args>` and parse its JSON output.
pub fn migrate_with_args<T: DeserializeOwned>(env: &TestEnv, args: &[&str]) -> T {
    let mut all_args = vec!["-C", &env.config_file_path, "--output", "json", "migrate"];
    all_args.extend_from_slice(args);
    serde_json::from_slice(&run_psigner(&all_args).stdout).unwrap()
}

/// Run a migration that must fail and return its stderr.
pub fn migrate_error(
    env: &TestEnv,
    kind: &str,
    account: &Address,
    new_owner: &Address,
    extra: &[&str],
) -> String {
    let account = account.to_string();
    let new_owner = new_owner.to_string();
    let mut args = vec![kind, &account, "--new-owner", &new_owner];
    args.extend_from_slice(extra);
    migrate_error_with_args(env, &args)
}

/// Run `migrate <args>`, which must fail, and return its stderr.
pub fn migrate_error_with_args(env: &TestEnv, args: &[&str]) -> String {
    let mut all_args = vec!["-C", &env.config_file_path, "migrate"];
    all_args.extend_from_slice(args);
    let output = run_psigner_with_input(&all_args, "");
    assert!(!output.status.success());
    String::from_utf8(output.stderr).unwrap()
}

/// Sign an execution message with the new owner, whose programmatic signer is the nonce
/// authority, then submit it with the forwarded signers signing the relay transaction.
pub async fn execute_as_new_owner(
    env: &TestEnv,
    execution_message: &str,
    nonce_account: &Address,
    new_owner: &Keypair,
    relay_signers: &[&Keypair],
) {
    let nonce_account_arg = nonce_account.to_string();
    let nonce_authority = programmatic_signer(&new_owner.pubkey()).to_string();
    let nonce_hash = current_nonce(env, nonce_account).await.to_string();
    let authority = new_owner.pubkey().to_string();
    let new_owner_file = keypair_file(new_owner);
    let sign = run_psigner(&[
        "-C",
        &env.config_file_path,
        "--output",
        "json",
        "transaction",
        "sign",
        "--execution-message",
        execution_message,
        "--nonce-account",
        &nonce_account_arg,
        "--nonce-authority",
        &nonce_authority,
        "--nonce-hash",
        &nonce_hash,
        "--authority",
        &authority,
        "--signer",
        new_owner_file.path().to_str().unwrap(),
        "--quiet",
        "--yes",
    ]);
    let sign: SignOutput = serde_json::from_slice(&sign.stdout).unwrap();
    let [signature] = sign.signatures.as_slice() else {
        panic!("expected one authority signature");
    };
    let signature = format!("{}={}", signature.address, signature.signature);

    let relay_signer_files = relay_signers
        .iter()
        .map(|signer| keypair_file(signer))
        .collect::<Vec<_>>();
    let mut args = vec![
        "-C",
        &env.config_file_path,
        "transaction",
        "submit",
        "--authorization-message",
        &sign.authorization_message,
        "--signer",
        &signature,
        "--quiet",
        "--yes",
    ];
    for file in &relay_signer_files {
        args.extend(["--relay-signer", file.path().to_str().unwrap()]);
    }
    run_psigner(&args);
}
