use {
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_programmatic_signer_client::{TransactionPlan, build_transaction, sign_transaction},
    std::{
        fs,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

struct Fixture {
    dir: TempDir,
    authority: Keypair,
    transaction: VersionedTransaction,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let authority = Keypair::new();
        write_keypair_file(&authority, dir.path().join("cold.json")).unwrap();
        write_keypair_file(&Keypair::new(), dir.path().join("payer.json")).unwrap();
        let transaction = build_transaction(
            &TransactionPlan::cancellation(Address::new_unique(), authority.pubkey()).unwrap(),
            Hash::new_from_array([1; 32]),
            Hash::new_from_array([2; 32]),
        )
        .unwrap();
        let fixture = Self {
            dir,
            authority,
            transaction,
        };
        fixture.save("tx.psigner", &fixture.transaction);
        fixture
    }
    fn path(&self, file: &str) -> String {
        self.dir.path().join(file).to_str().unwrap().into()
    }
    fn save(&self, file: &str, transaction: &VersionedTransaction) {
        fs::write(
            self.path(file),
            STANDARD.encode(wincode::serialize(transaction).unwrap()),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
            .args(["-u", "http://127.0.0.1:1"])
            .args(args)
            .output()
            .unwrap()
    }
    fn succeeds(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn fails(&self, args: &[&str], expected: &str) {
        let output = self.run(args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn offline_relay_assembly_and_signature_validation() {
    let mut fixture = Fixture::new();
    sign_transaction(&mut fixture.transaction, &fixture.authority).unwrap();
    fixture.save("signed.psigner", &fixture.transaction);
    let output = fixture.succeeds(&[
        "--fee-payer",
        &fixture.path("payer.json"),
        "transaction",
        "submit",
        &fixture.path("signed.psigner"),
        "--no-send",
        "--blockhash",
        &Hash::new_from_array([3; 32]).to_string(),
    ]);
    let bytes = STANDARD
        .decode(String::from_utf8(output.stdout).unwrap().trim())
        .unwrap();
    let relay: VersionedTransaction = wincode::deserialize_exact(&bytes).unwrap();
    relay.verify_and_hash_message().unwrap();
    fixture.transaction.signatures[0] = [42; 64].into();
    fixture.save("bad.psigner", &fixture.transaction);
    fixture.fails(
        &["transaction", "inspect", &fixture.path("bad.psigner")],
        "invalid signature",
    );
}

#[test]
fn batch_signing_preserves_existing_files_and_rejects_name_collisions() {
    let fixture = Fixture::new();
    fixture.save("second.psigner", &fixture.transaction);
    fs::create_dir(fixture.path("signed")).unwrap();
    fixture.succeeds(&[
        "transaction",
        "sign",
        &fixture.path("tx.psigner"),
        &fixture.path("second.psigner"),
        "--keypair",
        &fixture.path("cold.json"),
        "--outdir",
        &fixture.path("signed"),
    ]);
    let original = fs::read(fixture.path("signed/tx.psigner")).unwrap();
    fixture.fails(
        &[
            "transaction",
            "sign",
            &fixture.path("tx.psigner"),
            "--keypair",
            &fixture.path("cold.json"),
            "--outfile",
            &fixture.path("signed/tx.psigner"),
        ],
        "already exists",
    );
    assert_eq!(
        fs::read(fixture.path("signed/tx.psigner")).unwrap(),
        original
    );
    fixture.fails(
        &[
            "transaction",
            "sign",
            &fixture.path("tx.psigner"),
            &fixture.path("second.psigner"),
            "--keypair",
            &fixture.path("cold.json"),
        ],
        "batch signing requires --outdir",
    );
}

#[test]
fn verify_rejects_wrong_cluster_without_rpc() {
    let fixture = Fixture::new();
    let summary = spl_programmatic_signer_client::inspect(&fixture.transaction).unwrap();
    fixture.fails(
        &[
            "transaction",
            "verify",
            &fixture.path("tx.psigner"),
            "--nonce-value",
            &Hash::new_from_array([1; 32]).to_string(),
            "--nonce-authority",
            &summary.inner_required_signers[0].to_string(),
            "--genesis-hash",
            &Hash::new_from_array([9; 32]).to_string(),
            "--allow-partial",
        ],
        "genesis hash mismatch",
    );
}

#[test]
fn every_workflow_has_parseable_help() {
    let fixture = Fixture::new();
    for command in [
        "create",
        "inspect",
        "sign",
        "merge",
        "verify",
        "simulate",
        "submit",
        "next-nonce",
    ] {
        fixture.succeeds(&["transaction", command, "--help"]);
    }
    fixture.succeeds(&["nonce", "advance", "--help"]);
}
