use {
    crate::common::helpers::{TestEnv, assert_failure, run_psigner},
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_hash::Hash,
    solana_keypair::{Keypair, write_keypair_file},
    solana_message::{AccountKeys, VersionedMessage, v1},
    solana_nonce::{state::State as SystemNonceState, versions::Versions},
    solana_signature::Signature,
    solana_signer::Signer,
    solana_stake_interface::{
        instruction as stake_instruction,
        state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
    },
    solana_system_interface::instruction::{advance_nonce_account, create_nonce_account},
    solana_transaction::{Transaction, versioned::VersionedTransaction},
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_ed25519_signer_interface::instruction::Instruction as SignerInstruction,
    spl_legacy_message_executor_interface::instruction::Instruction as ExecutorInstruction,
    spl_programmatic_signer_cli::NonceCreateOutput,
    std::{path::Path, process::Command},
};

fn read_prepared_message(path: &Path) -> v1::Message {
    let data: CliSignOnlyData = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let message: VersionedMessage =
        wincode::deserialize_exact(&BASE64_STANDARD.decode(data.message.unwrap()).unwrap())
            .unwrap();
    let VersionedMessage::V1(message) = message else {
        panic!("expected a v1 migration transaction")
    };
    assert_eq!(data.blockhash, message.lifetime_specifier.to_string());
    assert!(data.signers.is_empty());
    assert!(data.bad_sig.is_empty());
    assert_eq!(
        data.absent,
        message.account_keys[..usize::from(message.header.num_required_signatures)]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    message
}

fn replace_arg(args: &mut [String], name: &str, value: &str) {
    let target = args
        .iter_mut()
        .skip_while(|arg| arg.as_str() != name)
        .nth(1)
        .expect("argument must exist");
    *target = value.to_owned();
}

struct StakeFixture {
    current_authority: Address,
    stake: Address,
    legacy_nonce: Address,
    legacy_nonce_authority: Keypair,
    programmatic_nonce: Address,
}

impl StakeFixture {
    async fn new(env: &TestEnv, current_authority: &Keypair, lockup: Lockup) -> Self {
        let stake = Keypair::new();
        let legacy_nonce = Keypair::new();
        // A separate nonce authority checks that preparation includes every required signer.
        let legacy_nonce_authority = Keypair::new();
        let rent = env
            .rpc
            .get_minimum_balance_for_rent_exemption(SystemNonceState::size())
            .await
            .unwrap();
        let mut instructions = stake_instruction::create_account(
            &env.payer.pubkey(),
            &stake.pubkey(),
            &Authorized::auto(&current_authority.pubkey()),
            &lockup,
            2_000_000_000,
        );
        instructions.extend(create_nonce_account(
            &env.payer.pubkey(),
            &legacy_nonce.pubkey(),
            &legacy_nonce_authority.pubkey(),
            rent,
        ));
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &[&env.payer, &stake, &legacy_nonce],
            env.rpc.get_latest_blockhash().await.unwrap(),
        );
        env.rpc
            .send_and_confirm_transaction(&transaction)
            .await
            .unwrap();
        let created = run_psigner(&[
            "-C",
            &env.config_file_path,
            "--output",
            "json",
            "nonce",
            "create",
            "--cold-authority",
            &current_authority.pubkey().to_string(),
        ]);
        let created: NonceCreateOutput = serde_json::from_slice(&created.stdout).unwrap();
        wait_for_finalization(env, &created.signature.parse().unwrap()).await;
        Self {
            current_authority: current_authority.pubkey(),
            stake: stake.pubkey(),
            legacy_nonce: legacy_nonce.pubkey(),
            legacy_nonce_authority,
            programmatic_nonce: created.nonce_account.parse().unwrap(),
        }
    }

    fn args(&self, config: &str, selection: &str, outfile: &Path) -> Vec<String> {
        [
            "-C",
            config,
            "migrate",
            "prepare",
            "stake",
            &self.stake.to_string(),
            "--authority",
            &self.current_authority.to_string(),
            "--authority-type",
            selection,
            "--legacy-nonce",
            &self.legacy_nonce.to_string(),
            "--programmatic-nonce",
            &self.programmatic_nonce.to_string(),
            "--outfile",
            outfile.to_str().unwrap(),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    fn prepare(&self, env: &TestEnv, selection: &str) -> v1::Message {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("migration.json");
        let args = self.args(&env.config_file_path, selection, &path);
        run_psigner(&args.iter().map(String::as_str).collect::<Vec<_>>());
        read_prepared_message(&path)
    }

    fn new_authority(&self) -> Address {
        ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &self.current_authority,
        )
    }
}

async fn wait_for_finalization(env: &TestEnv, signature: &Signature) {
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        loop {
            if let Some(result) = env
                .rpc
                .get_signature_status_with_commitment(
                    signature,
                    solana_commitment_config::CommitmentConfig::finalized(),
                )
                .await
                .unwrap()
            {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();
}

pub async fn prepares_selected_stake_authorities(env: &TestEnv) {
    let current_authority = Keypair::new();
    let fixture = StakeFixture::new(env, &current_authority, Lockup::default()).await;
    let addresses = [
        fixture.stake,
        fixture.legacy_nonce,
        fixture.programmatic_nonce,
    ];
    let before = env.rpc.get_multiple_accounts(&addresses).await.unwrap();
    let versions: Versions = wincode::deserialize(&before[1].as_ref().unwrap().data).unwrap();
    let SystemNonceState::Initialized(legacy_nonce) = versions.state() else {
        panic!("nonce is initialized")
    };
    let programmatic_nonce =
        spl_nonce_interface::state::Nonce::view(&before[2].as_ref().unwrap().data).unwrap();
    let new_authority = fixture.new_authority();

    for (selection, roles) in [
        ("staker", vec![StakeAuthorize::Staker]),
        ("withdrawer", vec![StakeAuthorize::Withdrawer]),
        (
            "both",
            vec![StakeAuthorize::Staker, StakeAuthorize::Withdrawer],
        ),
    ] {
        let message = fixture.prepare(env, selection);
        assert_eq!(message.account_keys[0], env.payer.pubkey());
        assert_eq!(message.lifetime_specifier, legacy_nonce.blockhash());
        assert_eq!(message.instructions.len(), 2);
        assert_eq!(message.config.compute_unit_limit, Some(400_000));
        assert_eq!(
            message.config.loaded_accounts_data_size_limit,
            Some(64 * 1024 * 1024)
        );
        assert_eq!(message.config.priority_fee.unwrap_or_default(), 0);
        let advance = advance_nonce_account(&fixture.legacy_nonce, &legacy_nonce.authority);
        assert_eq!(
            message.instructions[0],
            AccountKeys::new(&message.account_keys, None)
                .try_compile_instructions(&[advance])
                .unwrap()[0]
        );

        // The outer transaction uses the legacy nonce, while Execute carries the programmatic nonce.
        let SignerInstruction::Submit {
            signatures,
            message: approval,
        } = SignerInstruction::try_from_bytes(&message.instructions[1].data).unwrap();
        assert_eq!(signatures, [Signature::default()]);
        assert_eq!(approval.recent_blockhash(), &Hash::default());
        assert_eq!(approval.header().num_required_signatures, 1);
        assert_eq!(approval.static_account_keys()[0], fixture.current_authority);
        assert_eq!(approval.instructions().len(), 1);
        let execution = &approval.instructions()[0];
        assert_eq!(
            approval.static_account_keys()[usize::from(execution.program_id_index)],
            spl_legacy_message_executor_interface::id()
        );
        assert_eq!(
            approval.static_account_keys()[usize::from(execution.accounts[0])],
            fixture.programmatic_nonce
        );
        let ExecutorInstruction::Execute(inner) =
            ExecutorInstruction::try_from_bytes(&execution.data).unwrap();
        assert_eq!(inner.recent_blockhash, programmatic_nonce.nonce);
        assert_eq!(inner.account_keys[0], new_authority);
        assert_eq!(inner.instructions.len(), roles.len());
        for (instruction, role) in inner.instructions.iter().zip(roles) {
            let expected = stake_instruction::authorize_checked(
                &fixture.stake,
                &fixture.current_authority,
                &new_authority,
                role,
                None,
            );
            assert_eq!(*instruction, inner.compile_instruction(&expected));
        }
        let mut expected_signers = vec![
            env.payer.pubkey(),
            fixture.current_authority,
            legacy_nonce.authority,
        ];
        expected_signers.sort();
        let mut actual_signers =
            message.account_keys[..usize::from(message.header.num_required_signatures)].to_vec();
        actual_signers.sort();
        assert_eq!(actual_signers, expected_signers);
    }
    assert_eq!(
        env.rpc.get_multiple_accounts(&addresses).await.unwrap(),
        before
    );
}

pub async fn prepared_migration_executes_with_sdk_signatures(env: &TestEnv) {
    let current_authority = Keypair::new();
    let fixture = StakeFixture::new(env, &current_authority, Lockup::default()).await;
    let votes = env.rpc.get_vote_accounts().await.unwrap();
    let vote = votes.current.first().unwrap().vote_pubkey.parse().unwrap();
    let delegation = Transaction::new_signed_with_payer(
        &[stake_instruction::delegate_stake(
            &fixture.stake,
            &fixture.current_authority,
            &vote,
        )],
        Some(&env.payer.pubkey()),
        &[&env.payer, &current_authority],
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&delegation)
        .await
        .unwrap();
    let before = env.rpc.get_account(&fixture.stake).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("migration.json");
    let mut args = fixture.args(&env.config_file_path, "withdrawer", &path);
    args.extend(["--compute-unit-limit", "500000", "--priority-fee", "10000"].map(str::to_owned));
    run_psigner(&args.iter().map(String::as_str).collect::<Vec<_>>());
    let mut message = read_prepared_message(&path);
    assert_eq!(env.rpc.get_account(&fixture.stake).await.unwrap(), before);
    assert_eq!(message.config.compute_unit_limit, Some(500_000));
    assert_eq!(message.config.priority_fee, Some(10_000));

    // Embed approvals before signing the outer message because they change its bytes.
    let SignerInstruction::Submit {
        mut signatures,
        message: approval,
    } = SignerInstruction::try_from_bytes(&message.instructions[1].data).unwrap();
    signatures[0] = current_authority.sign_message(&approval.serialize());
    message.instructions[1].data = wincode::serialize(&SignerInstruction::Submit {
        signatures,
        message: approval,
    })
    .unwrap();
    let transaction = VersionedTransaction::try_new(
        VersionedMessage::V1(message),
        &[
            &env.payer,
            &current_authority,
            &fixture.legacy_nonce_authority,
        ],
    )
    .unwrap();
    env.rpc
        .send_and_confirm_transaction(&transaction)
        .await
        .unwrap();

    let after = env.rpc.get_account(&fixture.stake).await.unwrap();
    let state: StakeStateV2 = wincode::deserialize(&after.data).unwrap();
    assert_eq!(
        state.authorized().unwrap(),
        Authorized {
            staker: fixture.current_authority,
            withdrawer: fixture.new_authority()
        }
    );
    assert_eq!(state.lockup().unwrap(), Lockup::default());
    assert_eq!(after.lamports, before.lamports);
}

pub async fn prepares_and_executes_stake_lockups(env: &TestEnv) {
    let current_authority = Keypair::new();
    let custodian = Keypair::new();
    let active_lockup = Lockup {
        unix_timestamp: i64::MAX,
        custodian: custodian.pubkey(),
        ..Lockup::default()
    };
    let expired_lockup = Lockup {
        unix_timestamp: 1,
        ..active_lockup
    };
    let shared_custodian_lockup = Lockup {
        custodian: current_authority.pubkey(),
        ..active_lockup
    };
    for (selection, lockup, approval_signers) in [
        ("staker", active_lockup, vec![&current_authority]),
        (
            "withdrawer",
            active_lockup,
            vec![&current_authority, &custodian],
        ),
        ("both", active_lockup, vec![&current_authority, &custodian]),
        ("both", shared_custodian_lockup, vec![&current_authority]),
        ("both", expired_lockup, vec![&current_authority]),
    ] {
        let fixture = StakeFixture::new(env, &current_authority, lockup).await;
        let mut message = fixture.prepare(env, selection);
        let SignerInstruction::Submit {
            mut signatures,
            message: approval,
        } = SignerInstruction::try_from_bytes(&message.instructions[1].data).unwrap();
        let expected_authorities: Vec<_> = approval_signers
            .iter()
            .map(|signer| signer.pubkey())
            .collect();
        assert_eq!(
            approval.header().num_required_signatures as usize,
            expected_authorities.len()
        );
        assert_eq!(
            &approval.static_account_keys()[..expected_authorities.len()],
            expected_authorities
        );
        assert_eq!(
            signatures,
            vec![Signature::default(); expected_authorities.len()]
        );

        // Check the prepared signer list directly, including deduplication of shared roles.
        let mut expected_outer_signers =
            vec![env.payer.pubkey(), fixture.legacy_nonce_authority.pubkey()];
        expected_outer_signers.extend(&expected_authorities);
        expected_outer_signers.sort();
        let mut actual_outer_signers =
            message.account_keys[..usize::from(message.header.num_required_signatures)].to_vec();
        actual_outer_signers.sort();
        assert_eq!(actual_outer_signers, expected_outer_signers);

        // Approve the wrapped message first, then sign the completed transaction through the SDK.
        signatures = approval_signers
            .iter()
            .map(|signer| signer.sign_message(&approval.serialize()))
            .collect();
        message.instructions[1].data = wincode::serialize(&SignerInstruction::Submit {
            signatures,
            message: approval,
        })
        .unwrap();
        let mut outer_signers = vec![&env.payer, &fixture.legacy_nonce_authority];
        outer_signers.extend(approval_signers);
        let transaction =
            VersionedTransaction::try_new(VersionedMessage::V1(message), &outer_signers).unwrap();
        env.rpc
            .send_and_confirm_transaction(&transaction)
            .await
            .unwrap();

        let account = env.rpc.get_account(&fixture.stake).await.unwrap();
        let state: StakeStateV2 = wincode::deserialize(&account.data).unwrap();
        assert_eq!(
            state.authorized().unwrap(),
            Authorized {
                staker: if selection == "withdrawer" {
                    fixture.current_authority
                } else {
                    fixture.new_authority()
                },
                withdrawer: if selection == "staker" {
                    fixture.current_authority
                } else {
                    fixture.new_authority()
                },
            }
        );
        assert_eq!(state.lockup().unwrap(), lockup);
    }
}

pub async fn rejects_invalid_migration_inputs(env: &TestEnv) {
    let current_authority = Keypair::new();
    let fixture = StakeFixture::new(env, &current_authority, Lockup::default()).await;
    let other = StakeFixture::new(env, &Keypair::new(), Lockup::default()).await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("migration.json");
    for (argument, value, error) in [
        (
            "--authority",
            Address::new_unique().to_string(),
            "authority does not match",
        ),
        (
            "stake",
            env.payer_address.clone(),
            "not owned by the Stake program",
        ),
        (
            "--legacy-nonce",
            fixture.stake.to_string(),
            "System Program durable nonce",
        ),
        (
            "--programmatic-nonce",
            fixture.legacy_nonce.to_string(),
            "SPL Nonce account",
        ),
        (
            "--legacy-nonce",
            Address::new_unique().to_string(),
            "was not found",
        ),
        (
            "--programmatic-nonce",
            other.programmatic_nonce.to_string(),
            "not the new authority",
        ),
    ] {
        let mut args = fixture.args(&env.config_file_path, "both", &path);
        replace_arg(&mut args, argument, &value);
        let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
            .args(args)
            .output()
            .unwrap();
        assert_failure(&output, error);
        assert!(
            !path.exists(),
            "invalid inputs must not produce a handoff artifact"
        );
    }

    // A different withdrawer only prevents migrations that select the withdrawer role.
    let change_withdrawer = Transaction::new_signed_with_payer(
        &[stake_instruction::authorize(
            &fixture.stake,
            &fixture.current_authority,
            &Address::new_unique(),
            StakeAuthorize::Withdrawer,
            None,
        )],
        Some(&env.payer.pubkey()),
        &[&env.payer, &current_authority],
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&change_withdrawer)
        .await
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_spl-programmatic-signer-cli"))
        .args(fixture.args(&env.config_file_path, "both", &path))
        .output()
        .unwrap();
    assert_failure(&output, "withdrawer authority does not match");
    assert!(!path.exists());
    fixture.prepare(env, "staker");
}

pub async fn resolves_migration_fee_payer(env: &TestEnv) {
    let current_authority = Keypair::new();
    let fixture = StakeFixture::new(env, &current_authority, Lockup::default()).await;
    let directory = tempfile::tempdir().unwrap();
    let keypair_path = directory.path().join("authority.json");
    write_keypair_file(&current_authority, &keypair_path).unwrap();
    let keypair_path = keypair_path.to_str().unwrap();
    let authority_address = fixture.current_authority.to_string();
    let path = directory.path().join("migration.json");
    let args = fixture.args(&env.config_file_path, "both", &path);
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    for (options, payer) in [
        (vec![], env.payer.pubkey()),
        (vec!["--keypair", keypair_path], fixture.current_authority),
        (
            vec!["--keypair", &authority_address],
            fixture.current_authority,
        ),
        (
            vec!["--keypair", keypair_path, "--fee-payer", &env.payer_address],
            env.payer.pubkey(),
        ),
    ] {
        run_psigner(&[args.clone(), options].concat());
        assert_eq!(read_prepared_message(&path).account_keys[0], payer);
    }

    // Preparing with a public payer address must not load the configured wallet.
    let config_path = directory.path().join("config.yml");
    let mut config = solana_cli_config::Config::load(&env.config_file_path).unwrap();
    config.keypair_path = directory
        .path()
        .join("missing-wallet.json")
        .display()
        .to_string();
    config.save(config_path.to_str().unwrap()).unwrap();
    let args = fixture.args(config_path.to_str().unwrap(), "both", &path);
    let mut args: Vec<_> = args.iter().map(String::as_str).collect();
    args.extend(["--fee-payer", &authority_address]);
    run_psigner(&args);
    assert_eq!(
        read_prepared_message(&path).account_keys[0],
        fixture.current_authority
    );
}

pub async fn writes_handoff_artifact_and_receipt(env: &TestEnv) {
    let fixture = StakeFixture::new(env, &Keypair::new(), Lockup::default()).await;
    let directory = tempfile::tempdir().unwrap();
    let expected = fixture.prepare(env, "both");
    let required_signers: Vec<_> = expected.account_keys
        [..usize::from(expected.header.num_required_signatures)]
        .iter()
        .map(ToString::to_string)
        .collect();
    for format in ["display", "json", "json-compact"] {
        let path = directory.path().join(format!("{format}.json"));
        let args = fixture.args(&env.config_file_path, "both", &path);
        let mut args: Vec<_> = args.iter().map(String::as_str).collect();
        args.extend(["--output", format]);
        let result = run_psigner(&args);
        if format == "display" {
            assert_eq!(
                String::from_utf8(result.stdout).unwrap(),
                format!(
                    "Saved migration: {}\nStake account: {}\nNew authority: {}\nRequired signers: \
                     {}\n",
                    path.display(),
                    fixture.stake,
                    fixture.new_authority(),
                    required_signers.join(", ")
                )
            );
        } else {
            let receipt: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(
                receipt,
                serde_json::json!({
                    "path": path,
                    "stakeAccount": fixture.stake.to_string(),
                    "newAuthority": fixture.new_authority().to_string(),
                    "requiredSigners": required_signers,
                })
            );
        }
        assert_eq!(read_prepared_message(&path), expected);
    }
}
