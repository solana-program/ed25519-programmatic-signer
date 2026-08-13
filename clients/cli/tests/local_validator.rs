use {
    assert_cmd::Command,
    solana_keypair::{Keypair, write_keypair},
    solana_signer::Signer as _,
    std::{env, fs, path::Path},
    tempfile::TempDir,
};

// Run manually after starting a local validator with the SPL Nonce program loaded:
//
//   make build-sbf-nonce-program
//   solana-test-validator --reset \
//     --bpf-program Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB target/deploy/spl_nonce_program.so
//   solana airdrop 10 --keypair ~/.config/solana/id.json --url localhost
//   PSIGNER_LOCAL_VALIDATOR_FEE_PAYER=~/.config/solana/id.json \
//     cargo test -p psigner --test local_validator -- --ignored
#[test]
#[ignore = "requires a running local validator, deployed SPL programs, and a funded fee payer"]
fn nonce_create_and_show_against_local_validator() {
    let temp_dir = TempDir::new().unwrap();
    let nonce_keypair = Keypair::new();
    let authority = Keypair::new();
    let nonce_keypair_path = temp_dir.path().join("nonce.json");
    write_keypair_file(&nonce_keypair_path, &nonce_keypair);

    let fee_payer = env::var("PSIGNER_LOCAL_VALIDATOR_FEE_PAYER")
        .expect("PSIGNER_LOCAL_VALIDATOR_FEE_PAYER must point to a funded keypair");
    let url = env::var("PSIGNER_LOCAL_VALIDATOR_URL")
        .unwrap_or_else(|_| String::from("http://127.0.0.1:8899"));

    psigner()
        .args([
            "--url",
            &url,
            "nonce",
            "create",
            "--nonce-authority",
            &authority.pubkey().to_string(),
            "--nonce-keypair",
            path_str(&nonce_keypair_path),
            "--fee-payer",
            &fee_payer,
        ])
        .assert()
        .success();

    psigner()
        .args([
            "--url",
            &url,
            "nonce",
            "show",
            &nonce_keypair.pubkey().to_string(),
        ])
        .assert()
        .success();
}

fn psigner() -> Command {
    Command::cargo_bin("psigner").unwrap()
}

fn write_keypair_file(path: &Path, keypair: &Keypair) {
    let mut file = fs::File::create(path).unwrap();
    write_keypair(keypair, &mut file).unwrap();
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}
