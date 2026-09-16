use {
    crate::common::{
        approval::sign_only,
        helpers::{TestEnv, assert_failure, run_psigner, run_psigner_with_input},
    },
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::{VersionedMessage, legacy::Message},
    solana_signer::Signer,
    solana_system_interface::instruction::{create_account, transfer},
    solana_transaction::Transaction,
    spl_ed25519_signer_client::{ProgrammaticSigner, message::wrapped_message},
    spl_legacy_message_executor_client::instruction::execute,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_cli::NonceCreateOutput,
    std::{fs, process::Output},
    tempfile::NamedTempFile,
};

const INITIAL_BALANCE: u64 = 10_000_000;
const TRANSFER_AMOUNT: u64 = 1_000_000;

struct ExecutionTest {
    authority: Keypair,
    recipient: Address,
    nonce_account: Address,
    nonce: Nonce,
    inner: Message,
}

impl ExecutionTest {
    async fn new(env: &TestEnv) -> Self {
        let authority = Keypair::new();
        let programmatic_signer = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &authority.pubkey(),
        );
        let recipient = Keypair::new().pubkey();
        let create = run_psigner(&[
            "-C",
            &env.config_file_path,
            "nonce",
            "create",
            "--nonce-authority",
            &programmatic_signer.to_string(),
            "--output",
            "json-compact",
        ]);
        let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();
        let nonce_account: Address = create.nonce_account.parse().unwrap();
        let nonce = read_nonce(env, &nonce_account).await;

        let funding = [programmatic_signer, recipient]
            .map(|address| transfer(&env.payer.pubkey(), &address, INITIAL_BALANCE));
        let transaction = Transaction::new_signed_with_payer(
            &funding,
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.rpc.get_latest_blockhash().await.unwrap(),
        );
        env.rpc
            .send_and_confirm_transaction(&transaction)
            .await
            .unwrap();

        let inner = Message::new_with_blockhash(
            &[transfer(&programmatic_signer, &recipient, TRANSFER_AMOUNT)],
            Some(&programmatic_signer),
            &nonce.nonce,
        );
        Self {
            authority,
            recipient,
            nonce_account,
            nonce,
            inner,
        }
    }

    fn approval(&self) -> VersionedMessage {
        wrapped_message(
            &execute(&self.nonce_account, &self.inner),
            &[self.authority.pubkey()],
        )
    }

    fn submit(&self, env: &TestEnv, extra: &[&str]) -> Output {
        let approval = self.approval();
        let mut data = sign_only(&approval);
        data.signers.push(format!(
            "{}={}",
            self.authority.pubkey(),
            self.authority.sign_message(&approval.serialize())
        ));
        submit_approval(env, &data, extra)
    }
}

fn submit_approval(env: &TestEnv, data: &CliSignOnlyData, extra: &[&str]) -> Output {
    let file = NamedTempFile::new().unwrap();
    let bytes = serde_json::to_vec(data).unwrap();
    fs::write(&file, &bytes).unwrap();
    let mut args = vec![
        "-C",
        &env.config_file_path,
        "tx",
        "submit",
        file.path().to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let output = run_psigner_with_input(&args, "");
    assert_eq!(fs::read(file).unwrap(), bytes);
    output
}

async fn read_nonce(env: &TestEnv, address: &Address) -> Nonce {
    Nonce::view(&env.rpc.get_account_data(address).await.unwrap())
        .unwrap()
        .clone()
}

pub async fn rejects_invalid_nonce_accounts_before_wallet_loading(env: &TestEnv) {
    let mut test = ExecutionTest::new(env).await;
    let uninitialized = Keypair::new();
    let transaction = Transaction::new_signed_with_payer(
        &[create_account(
            &env.payer.pubkey(),
            &uninitialized.pubkey(),
            env.nonce_rent_lamports,
            u64::try_from(Nonce::LEN).unwrap(),
            &spl_nonce_interface::id(),
        )],
        Some(&env.payer.pubkey()),
        &[&env.payer, &uninitialized],
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&transaction)
        .await
        .unwrap();

    let directory = tempfile::tempdir().unwrap();
    let missing_payer = directory.path().join("missing-payer.json");
    for (address, expected) in [
        (Keypair::new().pubkey(), "was not found"),
        (env.payer.pubkey(), "not the SPL Nonce program"),
        (uninitialized.pubkey(), "invalid SPL Nonce data"),
    ] {
        test.nonce_account = address;
        let output = test.submit(env, &["--fee-payer", missing_payer.to_str().unwrap()]);
        assert_failure(&output, expected);
    }
}

pub async fn rejects_nonce_authority_outside_inner_signers(env: &TestEnv) {
    let mut test = ExecutionTest::new(env).await;
    // A real nonce account whose authority appears only as the unsigned transfer recipient.
    let create = run_psigner(&[
        "-C",
        &env.config_file_path,
        "nonce",
        "create",
        "--nonce-authority",
        &test.recipient.to_string(),
        "--output",
        "json-compact",
    ]);
    let create: NonceCreateOutput = serde_json::from_slice(&create.stdout).unwrap();
    test.nonce_account = create.nonce_account.parse().unwrap();
    test.nonce = read_nonce(env, &test.nonce_account).await;
    test.inner.recent_blockhash = test.nonce.nonce;
    assert_failure(
        &test.submit(env, &[]),
        &format!(
            "execution message does not list nonce authority {} as a signer",
            test.nonce.authority
        ),
    );
}

pub async fn submits_approved_transfer_and_rejects_replay(env: &TestEnv) {
    let test = ExecutionTest::new(env).await;
    let output = test.submit(env, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        env.rpc.get_balance(&test.recipient).await.unwrap(),
        INITIAL_BALANCE.checked_add(TRANSFER_AMOUNT).unwrap()
    );

    // The nonce has advanced, so the same approval cannot be submitted again.
    let advanced = read_nonce(env, &test.nonce_account).await;
    let replay = test.submit(env, &[]);
    assert_failure(
        &replay,
        &format!(
            "approved message uses nonce {}, but nonce account {} currently has {}",
            test.nonce.nonce, test.nonce_account, advanced.nonce
        ),
    );
}

pub async fn submits_with_file_and_cli_signatures_in_authority_order(env: &TestEnv) {
    let test = ExecutionTest::new(env).await;
    let other_authority = Keypair::new();
    let mut authorities = [&test.authority, &other_authority];

    // Reverse the authorities so returning signatures in sorted address order would fail.
    authorities.sort_by_key(|authority| std::cmp::Reverse(authority.pubkey()));
    let [file_signer, cli_signer] = authorities;
    let approval = wrapped_message(
        &execute(&test.nonce_account, &test.inner),
        &[file_signer.pubkey(), cli_signer.pubkey()],
    );
    let approval_bytes = approval.serialize();
    let mut data = sign_only(&approval);
    data.signers.push(format!(
        "{}={}",
        file_signer.pubkey(),
        file_signer.sign_message(&approval_bytes)
    ));
    let cli_signer_entry = format!(
        "{}={}",
        cli_signer.pubkey(),
        cli_signer.sign_message(&approval_bytes)
    );
    let fee_payer_file = NamedTempFile::new().unwrap();
    write_keypair_file(&env.payer, &fee_payer_file).unwrap();
    let output = submit_approval(
        env,
        &data,
        &[
            "--signer",
            &cli_signer_entry,
            "--fee-payer",
            fee_payer_file.path().to_str().unwrap(),
            "--keypair",
            &file_signer.pubkey().to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    assert_eq!(
        env.rpc.get_balance(&test.recipient).await.unwrap(),
        INITIAL_BALANCE.checked_add(TRANSFER_AMOUNT).unwrap()
    );
}
