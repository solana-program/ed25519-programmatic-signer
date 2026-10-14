use {
    crate::{
        common::{execute::programmatic_signer, helpers::TestEnv},
        migration::{
            assert_signers, authorization, create_nonce, decode_execution_message,
            execute_as_new_owner, migrate, migrate_error,
        },
    },
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_stake_interface::{
        instruction::create_account,
        state::{Authorized, Lockup, StakeStateV2},
    },
    solana_transaction::Transaction,
    spl_programmatic_signer_cli::StakeMigrationOutput,
};

const STAKE_LAMPORTS: u64 = 1_000_000_000;

async fn create_stake_account(env: &TestEnv, authorized: &Authorized, lockup: &Lockup) -> Address {
    let stake = Keypair::new();
    let transaction = Transaction::new_signed_with_payer(
        &create_account(
            &env.payer.pubkey(),
            &stake.pubkey(),
            authorized,
            lockup,
            STAKE_LAMPORTS,
        ),
        Some(&env.payer.pubkey()),
        &[&env.payer, &stake],
        env.rpc.get_latest_blockhash().await.unwrap(),
    );
    env.rpc
        .send_and_confirm_transaction(&transaction)
        .await
        .unwrap();
    stake.pubkey()
}

async fn stake_authorized(env: &TestEnv, stake: &Address) -> Authorized {
    let data = env.rpc.get_account_data(stake).await.unwrap();
    wincode::deserialize::<StakeStateV2>(&data)
        .unwrap()
        .authorized()
        .unwrap()
}

fn migrate_stake(
    env: &TestEnv,
    stake: &Address,
    new_owner: &Address,
    extra: &[&str],
) -> StakeMigrationOutput {
    migrate(env, "stake", stake, new_owner, extra)
}

fn migrate_stake_error(
    env: &TestEnv,
    stake: &Address,
    new_owner: &Address,
    extra: &[&str],
) -> String {
    migrate_error(env, "stake", stake, new_owner, extra)
}

async fn execute_migration(
    env: &TestEnv,
    migration: &StakeMigrationOutput,
    nonce_account: &Address,
    new_owner: &Keypair,
    relay_signers: &[&Keypair],
) {
    execute_as_new_owner(
        env,
        &migration.execution_message,
        nonce_account,
        new_owner,
        relay_signers,
    )
    .await;
}

pub async fn migrates_stake_to_new_owner(env: &TestEnv) {
    let old_owner = Keypair::new();
    let new_owner = Keypair::new();
    let new_owner_signer = programmatic_signer(&new_owner.pubkey());
    let stake = create_stake_account(
        env,
        &Authorized::auto(&old_owner.pubkey()),
        &Lockup::default(),
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    let migration = migrate_stake(env, &stake, &new_owner.pubkey(), &[]);
    assert_eq!(migration.authority_types, ["staker", "withdrawer"]);
    assert_eq!(migration.new_owner, new_owner_signer.to_string());
    assert_eq!(
        migration.new_owner_authority,
        new_owner.pubkey().to_string()
    );
    assert_eq!(migration.custodian, None);
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[old_owner.pubkey()],
    );
    // The new owner's programmatic signer stands in as the fee payer, and `transaction sign` sets
    // the lifetime.
    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(message.account_keys[0], new_owner_signer);
    assert_eq!(message.lifetime_specifier, Hash::default());

    execute_migration(env, &migration, &nonce_account, &new_owner, &[&old_owner]).await;
    assert_eq!(
        stake_authorized(env, &stake).await,
        Authorized::auto(&new_owner_signer)
    );
}

pub async fn migrates_stake_authorities_one_at_a_time(env: &TestEnv) {
    // The owner moves the stake account to its own programmatic signer.
    let owner = Keypair::new();
    let owner_signer = programmatic_signer(&owner.pubkey());
    let stake =
        create_stake_account(env, &Authorized::auto(&owner.pubkey()), &Lockup::default()).await;
    let nonce_account = create_nonce(env, &owner.pubkey());

    let staker = migrate_stake(
        env,
        &stake,
        &owner.pubkey(),
        &["--authority-type", "staker"],
    );
    assert_eq!(staker.authority_types, ["staker"]);
    assert_signers(
        &staker.required_signers,
        &[authorization(&owner.pubkey())],
        &[owner.pubkey()],
    );
    execute_migration(env, &staker, &nonce_account, &owner, &[&owner]).await;
    assert_eq!(
        stake_authorized(env, &stake).await,
        Authorized {
            staker: owner_signer,
            withdrawer: owner.pubkey(),
        }
    );

    let stderr = migrate_stake_error(
        env,
        &stake,
        &owner.pubkey(),
        &["--authority-type", "staker"],
    );
    assert!(
        stderr.contains(&format!(
            "the staker of stake account {stake} is already {owner_signer}"
        )),
        "{stderr}"
    );

    // The staker already moved, so moving both is rejected too.
    let stderr = migrate_stake_error(env, &stake, &owner.pubkey(), &[]);
    assert!(stderr.contains("is already"), "{stderr}");

    let withdrawer = migrate_stake(
        env,
        &stake,
        &owner.pubkey(),
        &["--authority-type", "withdrawer"],
    );
    execute_migration(env, &withdrawer, &nonce_account, &owner, &[&owner]).await;
    assert_eq!(
        stake_authorized(env, &stake).await,
        Authorized::auto(&owner_signer)
    );
}

pub async fn migrates_stake_with_lockup_custodian(env: &TestEnv) {
    let staker = Keypair::new();
    let withdrawer = Keypair::new();
    let custodian = Keypair::new();
    let new_owner = Keypair::new();
    let new_owner_signer = programmatic_signer(&new_owner.pubkey());
    let stake = create_stake_account(
        env,
        &Authorized {
            staker: staker.pubkey(),
            withdrawer: withdrawer.pubkey(),
        },
        &Lockup {
            unix_timestamp: 0,
            epoch: u64::MAX,
            custodian: custodian.pubkey(),
        },
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // Only moving the withdrawer needs the custodian.
    let staker_only = migrate_stake(
        env,
        &stake,
        &new_owner.pubkey(),
        &["--authority-type", "staker"],
    );
    assert_eq!(staker_only.custodian, None);

    let migration = migrate_stake(env, &stake, &new_owner.pubkey(), &[]);
    assert_eq!(migration.custodian, Some(custodian.pubkey().to_string()));
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[staker.pubkey(), withdrawer.pubkey(), custodian.pubkey()],
    );

    execute_migration(
        env,
        &migration,
        &nonce_account,
        &new_owner,
        &[&staker, &withdrawer, &custodian],
    )
    .await;
    assert_eq!(
        stake_authorized(env, &stake).await,
        Authorized::auto(&new_owner_signer)
    );
}

pub async fn rejects_invalid_stake_migrations(env: &TestEnv) {
    let owner = Keypair::new();
    let stake =
        create_stake_account(env, &Authorized::auto(&owner.pubkey()), &Lockup::default()).await;

    // Passing the programmatic signer in place of its authority.
    let owner_signer = programmatic_signer(&owner.pubkey());
    let stderr = migrate_stake_error(env, &stake, &owner_signer, &[]);
    assert!(
        stderr.contains(&format!(
            "new owner {owner_signer} is not an on-curve address"
        )),
        "{stderr}"
    );

    let stderr = migrate_stake_error(env, &env.payer.pubkey(), &owner.pubkey(), &[]);
    assert!(
        stderr.contains(&format!(
            "account {} is owned by 11111111111111111111111111111111, not the Stake Program",
            env.payer.pubkey()
        )),
        "{stderr}"
    );
}
