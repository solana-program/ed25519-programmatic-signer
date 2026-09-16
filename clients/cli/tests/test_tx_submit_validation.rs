use {
    crate::common::{
        approval::{approval_message, sign_only},
        helpers::{assert_failure, run_psigner_with_input},
    },
    solana_cli_config::Config as SolanaConfig,
    solana_cli_output::CliSignOnlyData,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::VersionedMessage,
    solana_signature::Signature,
    solana_signer::Signer,
    std::{fs, path::PathBuf, process::Output},
    tempfile::TempDir,
    test_case::test_case,
};

pub mod common;

struct SubmitTestEnv {
    directory: TempDir,
    config: PathBuf,
    input: PathBuf,
    authorities: [Keypair; 2],
    message: VersionedMessage,
}

impl SubmitTestEnv {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let authorities = [
            Keypair::new_from_array([1; 32]),
            Keypair::new_from_array([4; 32]),
        ];
        let message = VersionedMessage::Legacy(approval_message(
            &authorities.each_ref().map(Signer::pubkey),
        ));
        let config = directory.path().join("config.yml");
        // Local failures must precede both wallet access and any RPC request.
        SolanaConfig {
            json_rpc_url: "http://127.0.0.1:1".into(),
            keypair_path: directory
                .path()
                .join("missing-payer.json")
                .to_str()
                .unwrap()
                .into(),
            ..SolanaConfig::default()
        }
        .save(config.to_str().unwrap())
        .unwrap();
        let env = Self {
            input: directory.path().join("approval.json"),
            directory,
            config,
            authorities,
            message,
        };
        env.write(&sign_only(&env.message));
        env
    }

    fn write(&self, data: &CliSignOnlyData) {
        fs::write(&self.input, serde_json::to_vec(data).unwrap()).unwrap();
    }

    fn pairs(&self) -> Vec<String> {
        self.authorities
            .iter()
            .map(|authority| {
                format!(
                    "{}={}",
                    authority.pubkey(),
                    authority.sign_message(&self.message.serialize())
                )
            })
            .collect()
    }

    fn run(&self, extra: &[&str]) -> Output {
        let original = fs::read(&self.input).unwrap();
        let mut args = vec![
            "-C",
            self.config.to_str().unwrap(),
            "tx",
            "submit",
            self.input.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let output = run_psigner_with_input(&args, "");
        assert_eq!(fs::read(&self.input).unwrap(), original);
        output
    }

    fn reject(&self, extra: &[&str], expected: &str) {
        let output = self.run(extra);
        assert_failure(&output, expected);
    }
}

#[test]
fn rejects_malformed_sign_only_json() {
    let env = SubmitTestEnv::new();
    fs::write(&env.input, b"not JSON").unwrap();
    env.reject(&[], "invalid sign-only JSON");
}

#[test_case(true, false; "cli_invalid_first")]
#[test_case(false, false; "cli_invalid_last")]
#[test_case(true, true; "file_invalid_cli_valid")]
#[test_case(false, true; "file_valid_cli_invalid")]
fn rejects_invalid_signatures_even_with_valid_duplicates(invalid_first: bool, split_sources: bool) {
    let env = SubmitTestEnv::new();
    let mut pairs = [
        env.pairs()[0].clone(),
        format!("{}={}", env.authorities[0].pubkey(), Signature::default()),
    ];
    if invalid_first {
        pairs.reverse();
    }
    let mut args = vec![];
    if split_sources {
        let mut data = sign_only(&env.message);
        data.signers.push(pairs[0].clone());
        env.write(&data);
    } else {
        args.extend(["--signer", pairs[0].as_str()]);
    }
    args.extend(["--signer", pairs[1].as_str()]);
    env.reject(&args, "invalid approval signature");
}

#[test]
fn rejects_nondefault_approval_blockhash() {
    let mut env = SubmitTestEnv::new();
    env.message
        .set_recent_blockhash(Hash::new_from_array([9; 32]));
    let mut data = sign_only(&env.message);
    data.signers = env.pairs();
    env.write(&data);
    env.reject(&[], "outer message must use the default blockhash");
}

#[test]
fn rejects_signer_entries_without_equals() {
    let env = SubmitTestEnv::new();
    env.reject(
        &["--signer", "missing separator"],
        "invalid signer: expected ADDRESS=SIGNATURE",
    );
}

#[test]
fn rejects_cli_signatures_from_non_authorities() {
    let env = SubmitTestEnv::new();
    let outsider = Keypair::new();
    let pair = format!(
        "{}={}",
        outsider.pubkey(),
        outsider.sign_message(&env.message.serialize())
    );
    env.reject(&["--signer", &pair], "invalid approval signature");
}

#[test]
fn requires_fee_payer_authority_to_supply_approval_signature() {
    let env = SubmitTestEnv::new();
    env.reject(
        &[],
        &format!(
            "missing approval signature for {}",
            env.authorities[0].pubkey()
        ),
    );
    let authority_file = env.directory.path().join("authority.json");
    write_keypair_file(&env.authorities[1], &authority_file).unwrap();
    env.reject(
        &[
            "--signer",
            &env.pairs()[0],
            "--fee-payer",
            authority_file.to_str().unwrap(),
        ],
        &format!(
            "missing approval signature for {}",
            env.authorities[1].pubkey()
        ),
    );
}
