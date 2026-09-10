use {
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::legacy::Message,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction::{Transaction, versioned::VersionedTransaction},
    spl_ed25519_signer_client::{ProgrammaticSigner, message::wrapped_message},
    spl_legacy_message_executor_client::instruction::execute,
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
        let pda = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &authority.pubkey(),
        );
        let inner = Message::new_with_blockhash(&[], Some(&pda), &Hash::new_from_array([1; 32]));
        let mut message = wrapped_message(
            &execute(&Address::new_unique(), &inner),
            &[authority.pubkey()],
        );
        message.set_recent_blockhash(Hash::new_from_array([2; 32]));
        let transaction = VersionedTransaction {
            message,
            signatures: vec![Signature::default()],
        };
        let fixture = Self {
            dir,
            authority,
            transaction,
        };
        fixture.save("tx.json", &fixture.transaction);
        fixture
    }
    fn path(&self, file: &str) -> String {
        self.dir.path().join(file).to_str().unwrap().into()
    }
    fn save(&self, file: &str, transaction: &VersionedTransaction) {
        fs::write(
            self.path(file),
            serde_json::to_string(&transaction.clone().into_legacy_transaction().unwrap()).unwrap(),
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
fn offline_inspection_rejects_invalid_signatures() {
    let mut fixture = Fixture::new();
    fixture.transaction.signatures[0] = [42; 64].into();
    fixture.save("bad.json", &fixture.transaction);
    fixture.fails(
        &["transaction", "inspect", &fixture.path("bad.json")],
        "invalid signature",
    );
}

#[test]
fn batch_signing_preserves_existing_files_and_rejects_name_collisions() {
    let fixture = Fixture::new();
    fixture.save("second.json", &fixture.transaction);
    fs::create_dir(fixture.path("signed")).unwrap();
    fixture.succeeds(&[
        "transaction",
        "sign",
        &fixture.path("tx.json"),
        &fixture.path("second.json"),
        "--keypair",
        &fixture.path("cold.json"),
        "--outdir",
        &fixture.path("signed"),
    ]);
    let original = fs::read(fixture.path("signed/tx.json")).unwrap();
    // Files deserialize directly into the standard SDK type, with valid signatures
    // over the same message that was supplied before signing.
    let signed: Transaction = serde_json::from_slice(&original).unwrap();
    signed.verify().unwrap();
    assert_eq!(
        signed.message.serialize(),
        fixture.transaction.message.serialize()
    );
    fixture.fails(
        &[
            "transaction",
            "sign",
            &fixture.path("tx.json"),
            "--keypair",
            &fixture.path("cold.json"),
            "--outfile",
            &fixture.path("signed/tx.json"),
        ],
        "already exists",
    );
    assert_eq!(fs::read(fixture.path("signed/tx.json")).unwrap(), original);
    fixture.fails(
        &[
            "transaction",
            "sign",
            &fixture.path("tx.json"),
            &fixture.path("second.json"),
            "--keypair",
            &fixture.path("cold.json"),
        ],
        "batch signing requires --outdir",
    );
}

#[test]
fn verify_rejects_wrong_cluster_without_rpc() {
    let fixture = Fixture::new();
    let pda = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_client::id(),
        &fixture.authority.pubkey(),
    );
    fixture.fails(
        &[
            "transaction",
            "verify",
            &fixture.path("tx.json"),
            "--nonce-value",
            &Hash::new_from_array([1; 32]).to_string(),
            "--nonce-authority",
            &pda.to_string(),
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
