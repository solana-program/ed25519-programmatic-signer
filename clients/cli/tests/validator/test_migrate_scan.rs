use {
    crate::{
        common::{
            execute::programmatic_signer,
            helpers::{TestEnv, run_psigner},
        },
        migration::{migrate_error_with_args, migrate_with_args, send},
        test_migrate_mint::create_mint,
        test_migrate_program::deploy_program,
        test_migrate_stake::{STAKE_LAMPORTS, create_stake_account},
        test_migrate_token::{self, DECIMALS, mint_tokens},
        test_migrate_vote::create_vote_account,
    },
    solana_address::Address,
    solana_keypair::Keypair,
    solana_loader_v3_interface::get_program_data_address,
    solana_native_token::LAMPORTS_PER_SOL,
    solana_program_pack::Pack,
    solana_signer::Signer,
    solana_stake_interface::state::{Authorized, Lockup},
    solana_system_interface::instruction::{create_account, transfer},
    spl_programmatic_signer_cli::ScanOutput,
    spl_token_2022_interface::{
        instruction::{initialize_account3, mint_to_checked},
        state::Account as TokenAccount,
    },
};

/// Run `migrate scan <owner> [extra]` and parse its JSON output.
fn scan(env: &TestEnv, owner: &Address, extra: &[&str]) -> ScanOutput {
    let owner = owner.to_string();
    let mut args = vec!["scan", &owner];
    args.extend_from_slice(extra);
    migrate_with_args(env, &args)
}

/// Create a token account for `owner` at a new address, not its associated token account, and
/// mint `amount` to it.
async fn mint_to_new_account(
    env: &TestEnv,
    token_program: &Address,
    mint: &Address,
    owner: &Address,
    amount: u64,
) -> Address {
    let account = Keypair::new();
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(TokenAccount::LEN)
        .await
        .unwrap();
    send(
        env,
        &[
            create_account(
                &env.payer.pubkey(),
                &account.pubkey(),
                lamports,
                TokenAccount::LEN.try_into().unwrap(),
                token_program,
            ),
            initialize_account3(token_program, &account.pubkey(), mint, owner).unwrap(),
            mint_to_checked(
                token_program,
                mint,
                &account.pubkey(),
                &env.payer.pubkey(),
                &[],
                amount,
                DECIMALS,
            )
            .unwrap(),
        ],
        &[&account],
    )
    .await;
    account.pubkey()
}

pub async fn scans_owner_for_migrations(env: &TestEnv) {
    let owner = Keypair::new();
    let other = Keypair::new();
    let new_owner = Keypair::new().pubkey();
    let new_owner_arg = new_owner.to_string();
    let token = spl_token_2022_interface::inline_spl_token::id();
    let token_2022 = spl_token_2022_interface::id();

    // Nothing to migrate yet.
    let output = scan(env, &owner.pubkey(), &["--new-owner", &new_owner_arg]);
    assert_eq!(output.owner, owner.pubkey().to_string());
    assert!(output.stake.is_empty());
    assert!(output.vote.is_empty());
    assert!(output.program.is_empty());
    assert!(output.mint.is_empty());
    assert!(output.token.is_empty());
    assert!(output.sol.is_none());
    assert!(output.errors.is_empty());

    let both_roles =
        create_stake_account(env, &Authorized::auto(&owner.pubkey()), &Lockup::default()).await;
    let staker_only = create_stake_account(
        env,
        &Authorized {
            staker: owner.pubkey(),
            withdrawer: other.pubkey(),
        },
        &Lockup::default(),
    )
    .await;
    // Owned by someone else entirely.
    create_stake_account(env, &Authorized::auto(&other.pubkey()), &Lockup::default()).await;

    let vote = create_vote_account(env, &Keypair::new(), &owner.pubkey()).await;
    // The owner is the voter and node identity here, which don't migrate.
    create_vote_account(env, &owner, &other.pubkey()).await;

    // Pays the program's write transactions, which the owner signs.
    send(
        env,
        &[transfer(
            &env.payer.pubkey(),
            &owner.pubkey(),
            LAMPORTS_PER_SOL,
        )],
        &[],
    )
    .await;
    let program = deploy_program(env, &owner).await;

    let token_mint = create_mint(env, &token, &owner.pubkey(), Some(&owner.pubkey())).await;
    let token_2022_mint =
        create_mint(env, &token_2022, &owner.pubkey(), Some(&other.pubkey())).await;
    let freeze_only_mint =
        create_mint(env, &token_2022, &other.pubkey(), Some(&owner.pubkey())).await;

    let held_mint = test_migrate_token::create_mint(env, &token, &[]).await;
    let held_account = mint_tokens(env, &token, &held_mint, &owner.pubkey(), 150).await;
    let held_2022_mint = test_migrate_token::create_mint(env, &token_2022, &[]).await;
    let held_2022_account =
        mint_tokens(env, &token_2022, &held_2022_mint, &owner.pubkey(), 5).await;
    let other_account = mint_to_new_account(env, &token, &held_mint, &owner.pubkey(), 1_000).await;
    // An empty balance is not listed.
    let empty_mint = test_migrate_token::create_mint(env, &token, &[]).await;
    mint_tokens(env, &token, &empty_mint, &owner.pubkey(), 0).await;

    let output = scan(env, &owner.pubkey(), &["--new-owner", &new_owner_arg]);
    assert!(output.errors.is_empty(), "{:?}", output.errors);
    let command = |args: &str| format!("spl-programmatic-signer migrate {args}");

    let mut stake = output
        .stake
        .iter()
        .map(|item| {
            (
                item.stake_account.clone(),
                item.lamports,
                item.authority_types.clone(),
                item.command.clone(),
            )
        })
        .collect::<Vec<_>>();
    stake.sort();
    let mut expected_stake = vec![
        (
            both_roles.to_string(),
            STAKE_LAMPORTS,
            vec!["staker".to_string(), "withdrawer".to_string()],
            command(&format!("stake {both_roles} --new-owner {new_owner}")),
        ),
        (
            staker_only.to_string(),
            STAKE_LAMPORTS,
            vec!["staker".to_string()],
            command(&format!(
                "stake {staker_only} --new-owner {new_owner} --authority-type staker"
            )),
        ),
    ];
    expected_stake.sort();
    assert_eq!(stake, expected_stake);

    let [vote_item] = output.vote.as_slice() else {
        panic!("expected one vote account, found {:?}", output.vote);
    };
    assert_eq!(vote_item.vote_account, vote.to_string());
    assert_eq!(
        vote_item.command,
        command(&format!("vote {vote} --new-owner {new_owner}"))
    );

    let [program_item] = output.program.as_slice() else {
        panic!("expected one program, found {:?}", output.program);
    };
    assert_eq!(program_item.program, program.to_string());
    assert_eq!(
        program_item.program_data,
        get_program_data_address(&program).to_string()
    );
    assert_eq!(
        program_item.command,
        command(&format!("program {program} --new-owner {new_owner}"))
    );

    let mut mints = output
        .mint
        .iter()
        .map(|item| {
            (
                item.mint.clone(),
                item.token_program.clone(),
                item.authority_types.clone(),
                item.command.clone(),
            )
        })
        .collect::<Vec<_>>();
    mints.sort();
    let mut expected_mints = vec![
        (
            token_mint.to_string(),
            token.to_string(),
            vec!["mint".to_string(), "freeze".to_string()],
            command(&format!("mint {token_mint} --new-owner {new_owner}")),
        ),
        (
            token_2022_mint.to_string(),
            token_2022.to_string(),
            vec!["mint".to_string()],
            command(&format!(
                "mint {token_2022_mint} --new-owner {new_owner} --authority-type mint"
            )),
        ),
        (
            freeze_only_mint.to_string(),
            token_2022.to_string(),
            vec!["freeze".to_string()],
            command(&format!(
                "mint {freeze_only_mint} --new-owner {new_owner} --authority-type freeze"
            )),
        ),
    ];
    expected_mints.sort();
    assert_eq!(mints, expected_mints);

    let owner_arg = owner.pubkey();
    let mut tokens = output
        .token
        .iter()
        .map(|item| {
            (
                item.token_account.clone(),
                item.mint.clone(),
                item.token_program.clone(),
                item.amount.clone(),
                item.associated,
                item.command.clone(),
            )
        })
        .collect::<Vec<_>>();
    tokens.sort();
    let mut expected_tokens = vec![
        (
            held_account.to_string(),
            held_mint.to_string(),
            token.to_string(),
            "1.5".to_string(),
            true,
            Some(command(&format!(
                "token {held_mint} --owner {owner_arg} --new-owner {new_owner}"
            ))),
        ),
        (
            held_2022_account.to_string(),
            held_2022_mint.to_string(),
            token_2022.to_string(),
            "0.05".to_string(),
            true,
            Some(command(&format!(
                "token {held_2022_mint} --owner {owner_arg} --new-owner {new_owner}"
            ))),
        ),
        (
            other_account.to_string(),
            held_mint.to_string(),
            token.to_string(),
            "10".to_string(),
            false,
            None,
        ),
    ];
    expected_tokens.sort();
    assert_eq!(tokens, expected_tokens);

    let sol = output.sol.expect("the owner holds SOL");
    assert_eq!(
        sol.lamports,
        env.rpc.get_balance(&owner.pubkey()).await.unwrap()
    );
    assert_eq!(
        sol.command,
        command(&format!("sol --owner {owner_arg} --new-owner {new_owner}"))
    );

    // Without a new owner, the commands hold a placeholder.
    let output = scan(env, &owner.pubkey(), &[]);
    assert_eq!(
        output.program[0].command,
        command(&format!("program {program} --new-owner <NEW_OWNER>"))
    );

    let display = String::from_utf8(
        run_psigner(&[
            "-C",
            &env.config_file_path,
            "migrate",
            "scan",
            &owner.pubkey().to_string(),
        ])
        .stdout,
    )
    .unwrap();
    assert!(display.contains("Stake accounts (2):"), "{display}");
    assert!(
        display.contains("Vote accounts (withdrawer) (1):"),
        "{display}"
    );
    assert!(display.contains("Token balances (3):"), "{display}");
    assert!(
        display.contains(&format!(
            "In {other_account}, not an associated token account"
        )),
        "{display}"
    );
    assert!(!display.contains("Not scanned"), "{display}");
}

pub async fn rejects_invalid_scans(env: &TestEnv) {
    let new_owner = Keypair::new().pubkey();
    let new_owner_signer = programmatic_signer(&new_owner);
    let stderr = migrate_error_with_args(
        env,
        &[
            "scan",
            &new_owner_signer.to_string(),
            "--new-owner",
            &new_owner.to_string(),
        ],
    );
    assert!(
        stderr.contains(&format!(
            "owner {new_owner_signer} is already the new owner's programmatic signer"
        )),
        "{stderr}"
    );

    let stderr = migrate_error_with_args(
        env,
        &[
            "scan",
            &new_owner.to_string(),
            "--new-owner",
            &new_owner_signer.to_string(),
        ],
    );
    assert!(
        stderr.contains(&format!(
            "new owner {new_owner_signer} is not an on-curve address"
        )),
        "{stderr}"
    );
}
