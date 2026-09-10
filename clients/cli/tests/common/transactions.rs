use {
    super::helpers::{TestEnv, run_signer},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde_json::{Value, json},
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::legacy::Message,
    solana_program_pack::Pack,
    solana_signer::Signer,
    solana_system_interface::instruction::transfer,
    solana_transaction::Transaction,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_programmatic_signer_cli::output::{
        Inspection, NonceCreateOutput, SimulationOutput, SubmitOutput, VerifyOutput,
    },
    spl_token_interface::{
        instruction as token_instruction,
        state::{Account as TokenAccount, Mint},
    },
    std::{
        fs,
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

const OFFLINE_URL: &str = "http://127.0.0.1:1";
const TRANSFER: u64 = 1_000_000;

struct Demo<'a> {
    env: &'a TestEnv,
    dir: TempDir,
    authority: Keypair,
    recipient: Address,
    pda: Address,
    nonce_account: Address,
    nonce: Hash,
    genesis: Hash,
}

impl<'a> Demo<'a> {
    async fn new(env: &'a TestEnv) -> Self {
        Self::with_balance(env, 20_000_000).await
    }

    async fn with_balance(env: &'a TestEnv, lamports: u64) -> Self {
        let dir = TempDir::new().unwrap();
        let authority = Keypair::new();
        write_keypair_file(&authority, dir.path().join("cold.json")).unwrap();
        let pda = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &authority.pubkey(),
        );
        let recipient = Address::new_unique();
        let result = run_signer(&[
            "-C",
            &env.config_file_path,
            "--output",
            "json",
            "nonce",
            "create",
            "--cold-authority",
            &authority.pubkey().to_string(),
        ]);
        let created: NonceCreateOutput = serde_json::from_slice(&result.stdout).unwrap();
        let mut instructions = vec![transfer(&env.payer.pubkey(), &recipient, 1_000_000)];
        if lamports > 0 {
            instructions.push(transfer(&env.payer.pubkey(), &pda, lamports));
        }
        let mut funding = Transaction::new_with_payer(&instructions, Some(&env.payer.pubkey()));
        funding.sign(&[&env.payer], env.rpc.get_latest_blockhash().await.unwrap());
        env.rpc
            .send_and_confirm_transaction(&funding)
            .await
            .unwrap();
        Self {
            env,
            dir,
            authority,
            recipient,
            pda,
            nonce_account: created.nonce_account.parse().unwrap(),
            nonce: created.nonce.parse().unwrap(),
            genesis: env.rpc.get_genesis_hash().await.unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn file(&self, name: &str) -> String {
        self.path(name).to_str().unwrap().to_owned()
    }

    fn json(&self, args: &[&str]) -> Value {
        let args = [
            &["-C", &self.env.config_file_path, "--output", "json"][..],
            args,
        ]
        .concat();
        serde_json::from_slice(&run_signer(&args).stdout).unwrap()
    }

    fn typed_json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> T {
        serde_json::from_value(self.json(args)).unwrap()
    }

    fn command(&self, args: &[&str]) -> Output {
        run_signer(&[&["-C", &self.env.config_file_path][..], args].concat())
    }

    fn fail(&self, args: &[&str], expected: &str) {
        let result = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
            .args(["-C", &self.env.config_file_path])
            .args(args)
            .output()
            .unwrap();
        assert!(
            !result.status.success(),
            "command unexpectedly succeeded: {args:?}"
        );
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }

    fn source(&self, name: &str, nonce: Hash, amount: u64, payer: Option<Address>) {
        let message = Message::new_with_blockhash(
            &[transfer(&self.pda, &self.recipient, amount)],
            Some(&payer.unwrap_or(self.pda)),
            &nonce,
        );
        fs::write(self.path(name), json!({ "blockhash": nonce.to_string(), "message": STANDARD.encode(message.serialize()) }).to_string()).unwrap();
    }

    fn create(&self, source: &str, file: &str, extra: &[&str]) {
        self.command(
            &[
                &[
                    "transaction",
                    "create",
                    "--from-sign-only",
                    &self.file(source),
                    "--nonce",
                    &self.nonce_account.to_string(),
                    "--authority",
                    &self.authority.pubkey().to_string(),
                    "--outfile",
                    &self.file(file),
                ][..],
                extra,
            ]
            .concat(),
        );
    }

    fn sign(&self, source: &str, output: &str) {
        self.command(&[
            "-u",
            OFFLINE_URL,
            "transaction",
            "sign",
            &self.file(source),
            "--keypair",
            &self.file("cold.json"),
            "--outfile",
            &self.file(output),
        ]);
    }

    fn verify(&self, file: &str) -> VerifyOutput {
        self.typed_json(&["transaction", "verify", &self.file(file), "--fetch-nonce"])
    }

    fn submit(&self, file: &str) -> SubmitOutput {
        self.typed_json(&["transaction", "submit", &self.file(file)])
    }
}

pub async fn transfer_offline_signing_simulation_and_replay(env: &TestEnv) {
    let demo = Demo::new(env).await;
    demo.source("inner.json", demo.nonce, TRANSFER, None);
    demo.create("inner.json", "tx.json", &["--fetch-nonce"]);
    let inspected: Inspection = demo.typed_json(&[
        "-u",
        OFFLINE_URL,
        "transaction",
        "inspect",
        &demo.file("tx.json"),
    ]);
    assert_eq!(inspected.nonce_account, demo.nonce_account.to_string());
    assert!(
        inspected.inner_instructions[0]
            .description
            .contains(&format!("Transfer {TRANSFER} lamports"))
    );
    assert!(!inspected.transaction_signers[0].signed);
    demo.fail(
        &[
            "transaction",
            "verify",
            &demo.file("tx.json"),
            "--fetch-nonce",
        ],
        "not fully signed",
    );
    assert_eq!(
        demo.json(&[
            "transaction",
            "verify",
            &demo.file("tx.json"),
            "--fetch-nonce",
            "--allow-partial"
        ])["fullySigned"],
        false
    );
    assert_eq!(
        demo.json(&["transaction", "simulate", "inner", &demo.file("tx.json")])["mode"],
        "inner"
    );
    demo.sign("tx.json", "signed.json");
    assert_eq!(
        demo.json(&[
            "-u",
            OFFLINE_URL,
            "transaction",
            "verify",
            &demo.file("signed.json"),
            "--nonce-value",
            &demo.nonce.to_string(),
            "--nonce-authority",
            &demo.pda.to_string(),
            "--genesis-hash",
            &demo.genesis.to_string()
        ])["fullySigned"],
        true
    );
    assert!(demo.verify("signed.json").fully_signed);
    let simulation: SimulationOutput = demo.typed_json(&[
        "transaction",
        "simulate",
        "relay",
        &demo.file("signed.json"),
    ]);
    assert!(simulation.units_consumed.unwrap() > 0);
    let submitted = demo.submit("signed.json");
    assert_eq!(submitted.expected_next_nonce, submitted.observed_nonce);
    assert_eq!(submitted.observed_nonce, inspected.next_nonce);
    assert_eq!(
        env.rpc.get_balance(&demo.recipient).await.unwrap(),
        1_000_000_u64.saturating_add(TRANSFER)
    );
    demo.fail(
        &["transaction", "submit", &demo.file("signed.json")],
        "nonce mismatch",
    );
    demo.fail(
        &[
            "transaction",
            "sign",
            &demo.file("tx.json"),
            "--keypair",
            &demo.file("cold.json"),
            "--outfile",
            &demo.file("signed.json"),
        ],
        "already exists",
    );
}

pub async fn cancellation_invalidates_pending_file(env: &TestEnv) {
    let demo = Demo::new(env).await;
    demo.source("inner.json", demo.nonce, TRANSFER, None);
    demo.create("inner.json", "tx.json", &["--fetch-nonce"]);
    demo.sign("tx.json", "signed.json");
    demo.command(&[
        "-u",
        OFFLINE_URL,
        "nonce",
        "advance",
        "--from-transaction",
        &demo.file("tx.json"),
        "--authority",
        &demo.authority.pubkey().to_string(),
        "--outfile",
        &demo.file("cancel.json"),
    ]);
    demo.sign("cancel.json", "cancel.signed.json");
    demo.submit("cancel.signed.json");
    demo.fail(
        &["transaction", "submit", &demo.file("signed.json")],
        "nonce mismatch",
    );
    assert_eq!(
        env.rpc.get_balance(&demo.recipient).await.unwrap(),
        1_000_000
    );
}

pub async fn precomputed_chain_runs_in_order(env: &TestEnv) {
    let demo = Demo::new(env).await;
    demo.source("first.source.json", demo.nonce, TRANSFER, None);
    demo.create(
        "first.source.json",
        "first.json",
        &[
            "-u",
            OFFLINE_URL,
            "--nonce-value",
            &demo.nonce.to_string(),
            "--genesis-hash",
            &demo.genesis.to_string(),
        ],
    );
    let next = demo.json(&[
        "-u",
        OFFLINE_URL,
        "transaction",
        "inspect",
        &demo.file("first.json"),
    ]);
    let next = next["nextNonce"].as_str().unwrap().parse::<Hash>().unwrap();
    demo.source("second.source.json", next, TRANSFER, None);
    demo.create(
        "second.source.json",
        "second.json",
        &["-u", OFFLINE_URL, "--after", &demo.file("first.json")],
    );
    demo.sign("first.json", "first.signed.json");
    demo.sign("second.json", "second.signed.json");
    demo.fail(
        &["transaction", "submit", &demo.file("second.signed.json")],
        "nonce mismatch",
    );
    demo.submit("first.signed.json");
    demo.submit("second.signed.json");
    assert_eq!(
        env.rpc.get_balance(&demo.recipient).await.unwrap(),
        1_000_000_u64.saturating_add(TRANSFER.saturating_mul(2))
    );
}

pub async fn designated_relayer_merges_signatures_and_requires_live_signer(env: &TestEnv) {
    let demo = Demo::new(env).await;
    let relayer = Keypair::new();
    write_keypair_file(&relayer, demo.path("relayer.json")).unwrap();
    demo.source("inner.json", demo.nonce, TRANSFER, Some(relayer.pubkey()));
    demo.create(
        "inner.json",
        "tx.json",
        &[
            "--fetch-nonce",
            "--submit-signer",
            &relayer.pubkey().to_string(),
        ],
    );
    demo.sign("tx.json", "cold.partial.json");
    demo.command(&[
        "-u",
        OFFLINE_URL,
        "transaction",
        "sign",
        &demo.file("tx.json"),
        "--keypair",
        &demo.file("relayer.json"),
        "--outfile",
        &demo.file("relayer.partial.json"),
    ]);
    demo.command(&[
        "-u",
        OFFLINE_URL,
        "transaction",
        "merge",
        &demo.file("cold.partial.json"),
        &demo.file("relayer.partial.json"),
        "--outfile",
        &demo.file("ready.json"),
    ]);
    assert!(demo.verify("ready.json").fully_signed);
    demo.fail(
        &["transaction", "submit", &demo.file("ready.json")],
        "missing outer signer",
    );
    demo.json(&[
        "transaction",
        "submit",
        &demo.file("ready.json"),
        "--submit-signer",
        &demo.file("relayer.json"),
    ]);
    assert_eq!(
        env.rpc.get_balance(&demo.recipient).await.unwrap(),
        1_000_000_u64.saturating_add(TRANSFER)
    );
}

pub async fn failed_inner_simulation_and_submission_preserve_nonce(env: &TestEnv) {
    let demo = Demo::new(env).await;
    demo.source("inner.json", demo.nonce, 30_000_000, None);
    demo.create("inner.json", "tx.json", &["--fetch-nonce"]);
    demo.sign("tx.json", "signed.json");
    demo.fail(
        &[
            "transaction",
            "simulate",
            "relay",
            &demo.file("signed.json"),
        ],
        "simulation failed",
    );
    demo.fail(
        &[
            "--skip-preflight",
            "transaction",
            "submit",
            &demo.file("signed.json"),
        ],
        "failed to send",
    );
    assert_eq!(demo.verify("signed.json").nonce, demo.nonce.to_string());
    assert_eq!(
        env.rpc.get_balance(&demo.recipient).await.unwrap(),
        1_000_000
    );
}

pub async fn token_transfer_is_decoded_and_lands(env: &TestEnv) {
    let demo = Demo::with_balance(env, 0).await;
    assert_eq!(env.rpc.get_balance(&demo.pda).await.unwrap(), 0);
    let mint = Keypair::new();
    let source = Keypair::new();
    let destination = Keypair::new();
    let token_program = spl_token_interface::id();
    let initializations = [
        (
            &mint,
            Mint::LEN,
            token_instruction::initialize_mint2(
                &token_program,
                &mint.pubkey().to_bytes().into(),
                &env.payer.pubkey().to_bytes().into(),
                None,
                6,
            )
            .unwrap(),
        ),
        (
            &source,
            TokenAccount::LEN,
            token_instruction::initialize_account3(
                &token_program,
                &source.pubkey().to_bytes().into(),
                &mint.pubkey().to_bytes().into(),
                &demo.pda.to_bytes().into(),
            )
            .unwrap(),
        ),
        (
            &destination,
            TokenAccount::LEN,
            token_instruction::initialize_account3(
                &token_program,
                &destination.pubkey().to_bytes().into(),
                &mint.pubkey().to_bytes().into(),
                &demo.recipient.to_bytes().into(),
            )
            .unwrap(),
        ),
    ];
    for (keypair, len, initialize) in initializations {
        let rent = env
            .rpc
            .get_minimum_balance_for_rent_exemption(len)
            .await
            .unwrap();
        let create = solana_system_interface::instruction::create_account(
            &env.payer.pubkey(),
            &keypair.pubkey(),
            rent,
            len as u64,
            &token_program.to_bytes().into(),
        );
        let transaction = Transaction::new_signed_with_payer(
            &[create, initialize],
            Some(&env.payer.pubkey()),
            &[&env.payer, keypair],
            env.rpc.get_latest_blockhash().await.unwrap(),
        );
        env.rpc
            .send_and_confirm_transaction(&transaction)
            .await
            .unwrap();
    }
    let mint_to = token_instruction::mint_to(
        &token_program,
        &mint.pubkey().to_bytes().into(),
        &source.pubkey().to_bytes().into(),
        &env.payer.pubkey().to_bytes().into(),
        &[],
        2_000_000,
    )
    .unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[mint_to],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&transaction)
        .await
        .unwrap();
    let transfer = token_instruction::transfer_checked(
        &token_program,
        &source.pubkey().to_bytes().into(),
        &mint.pubkey().to_bytes().into(),
        &destination.pubkey().to_bytes().into(),
        &demo.pda.to_bytes().into(),
        &[],
        1_250_000,
        6,
    )
    .unwrap();
    let message = Message::new_with_blockhash(&[transfer], Some(&demo.pda), &demo.nonce);
    fs::write(demo.path("token.source.json"), json!({ "blockhash": demo.nonce.to_string(), "message": STANDARD.encode(message.serialize()) }).to_string()).unwrap();
    demo.create("token.source.json", "token.json", &["--fetch-nonce"]);
    let inspected: Inspection = demo.typed_json(&[
        "-u",
        OFFLINE_URL,
        "transaction",
        "inspect",
        &demo.file("token.json"),
    ]);
    let description = &inspected.inner_instructions[0].description;
    assert!(description.contains("1250000 raw token units (6 decimals)"));
    assert!(description.contains(&mint.pubkey().to_string()));
    demo.json(&["transaction", "simulate", "inner", &demo.file("token.json")]);
    demo.sign("token.json", "token.signed.json");
    demo.json(&[
        "transaction",
        "simulate",
        "relay",
        &demo.file("token.signed.json"),
    ]);
    demo.submit("token.signed.json");
    let destination = env.rpc.get_account(&destination.pubkey()).await.unwrap();
    assert_eq!(
        TokenAccount::unpack(&destination.data).unwrap().amount,
        1_250_000
    );
    let source = env.rpc.get_account(&source.pubkey()).await.unwrap();
    assert_eq!(TokenAccount::unpack(&source.data).unwrap().amount, 750_000);
}
