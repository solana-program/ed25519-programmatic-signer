use {
    crate::{
        common::{execute::programmatic_signer, helpers::TestEnv},
        migration::{
            assert_signers, authorization, create_nonce, decode_execution_message,
            execute_as_new_owner, migrate, migrate_error, send,
        },
    },
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_native_token::LAMPORTS_PER_SOL,
    solana_signer::Signer,
    solana_system_interface::instruction::{create_account, transfer},
    spl_associated_token_account_interface::{
        address::get_associated_token_address_with_program_id,
        instruction::create_associated_token_account_idempotent,
    },
    spl_programmatic_signer_cli::{Authorization, TokenMigrationOutput},
    spl_token_2022_interface::{
        extension::{
            ExtensionType, StateWithExtensions,
            cpi_guard::instruction::enable_cpi_guard,
            default_account_state::instruction::initialize_default_account_state,
            transfer_fee::instruction::{
                harvest_withheld_tokens_to_mint, initialize_transfer_fee_config,
            },
            transfer_hook,
        },
        instruction::{
            AuthorityType, close_account, freeze_account, initialize_mint2,
            initialize_non_transferable_mint, mint_to_checked, reallocate, set_authority,
            thaw_account, transfer_checked,
        },
        state::{Account as TokenAccount, AccountState, Mint},
    },
};

pub const DECIMALS: u8 = 2;

/// Create a mint with the test payer as its mint and freeze authority, and the given Token-2022
/// extensions.
pub async fn create_mint(
    env: &TestEnv,
    token_program: &Address,
    extensions: &[ExtensionType],
) -> Address {
    let mint = Keypair::new();
    let payer = env.payer.pubkey();
    let space = ExtensionType::try_calculate_account_len::<Mint>(extensions).unwrap();
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(space)
        .await
        .unwrap();
    let mut instructions = vec![create_account(
        &payer,
        &mint.pubkey(),
        lamports,
        space.try_into().unwrap(),
        token_program,
    )];
    for extension in extensions {
        instructions.push(
            match extension {
                ExtensionType::NonTransferable => {
                    initialize_non_transferable_mint(token_program, &mint.pubkey())
                }
                ExtensionType::TransferHook => transfer_hook::instruction::initialize(
                    token_program,
                    &mint.pubkey(),
                    None,
                    Some(Address::new_unique()),
                ),
                ExtensionType::DefaultAccountState => initialize_default_account_state(
                    token_program,
                    &mint.pubkey(),
                    &AccountState::Frozen,
                ),
                ExtensionType::TransferFeeConfig => initialize_transfer_fee_config(
                    token_program,
                    &mint.pubkey(),
                    None,
                    None,
                    100, // transfer fee basis points
                    u64::MAX,
                ),
                extension => panic!("unsupported extension {extension:?}"),
            }
            .unwrap(),
        );
    }
    instructions.push(
        initialize_mint2(
            token_program,
            &mint.pubkey(),
            &payer,
            Some(&payer),
            DECIMALS,
        )
        .unwrap(),
    );
    send(env, &instructions, &[&mint]).await;
    mint.pubkey()
}

/// Mint `amount` to `owner`'s associated token account, creating it, and return the account.
pub async fn mint_tokens(
    env: &TestEnv,
    token_program: &Address,
    mint: &Address,
    owner: &Address,
    amount: u64,
) -> Address {
    let payer = env.payer.pubkey();
    let account = get_associated_token_address_with_program_id(owner, mint, token_program);
    send(
        env,
        &[
            create_associated_token_account_idempotent(&payer, owner, mint, token_program),
            mint_to_checked(token_program, mint, &account, &payer, &[], amount, DECIMALS).unwrap(),
        ],
        &[],
    )
    .await;
    account
}

/// Give `address` SOL, to pay rent for the new authority's token account.
async fn fund(env: &TestEnv, address: &Address) {
    send(
        env,
        &[transfer(&env.payer.pubkey(), address, LAMPORTS_PER_SOL)],
        &[],
    )
    .await;
}

async fn token_balance(env: &TestEnv, account: &Address) -> u64 {
    let data = env.rpc.get_account_data(account).await.unwrap();
    StateWithExtensions::<TokenAccount>::unpack(&data)
        .unwrap()
        .base
        .amount
}

async fn assert_closed(env: &TestEnv, account: &Address) {
    let account = env
        .rpc
        .get_account_with_commitment(account, env.rpc.commitment())
        .await;
    assert_eq!(account.unwrap().value, None);
}

async fn migrates_tokens(env: &TestEnv, token_program: &Address) {
    let owner = Keypair::new();
    let owner_arg = owner.pubkey().to_string();
    let new_owner = Keypair::new();
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    fund(env, &owner.pubkey()).await;
    let mint = create_mint(env, token_program, &[]).await;
    let source = mint_tokens(env, token_program, &mint, &owner.pubkey(), 1_000).await;
    let destination = get_associated_token_address_with_program_id(
        &new_owner_programmatic_signer,
        &mint,
        token_program,
    );
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // Without a new authority, the tokens move to the owner's own programmatic signer.
    let default: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        None,
        &["--owner", &owner_arg, "--amount", "2.5"],
    );
    assert_eq!(default.new_authority_signing_pubkey, owner_arg);

    // Without an owner, the tokens move from the configured keypair's token account.
    let payer_source = mint_tokens(env, token_program, &mint, &env.payer.pubkey(), 1).await;
    let default_owner: TokenMigrationOutput = migrate(env, "token", &mint, None, &[]);
    assert_eq!(default_owner.source, payer_source.to_string());
    assert_eq!(
        default_owner.new_authority_signing_pubkey,
        env.payer_address
    );

    // The new authority has no token account yet, so the migration creates it, paid for by the
    // owner.
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg, "--amount", "2.5"],
    );
    assert_eq!(migration.mint, mint.to_string());
    assert_eq!(migration.token_program, token_program.to_string());
    assert_eq!(migration.amount, "2.5");
    assert_eq!(migration.source, source.to_string());
    assert_eq!(migration.destination, destination.to_string());
    assert_eq!(migration.funder, Some(owner_arg.clone()));
    assert!(!migration.transfer_fee);
    // Tokens are left behind, so the source stays open.
    assert_eq!(migration.rent_recipient, None);
    assert_eq!(
        migration.new_authority,
        new_owner_programmatic_signer.to_string()
    );
    assert_eq!(
        migration.new_authority_signing_pubkey,
        new_owner.pubkey().to_string()
    );
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[owner.pubkey()],
    );

    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(message.account_keys[0], new_owner_programmatic_signer);
    assert_eq!(message.lifetime_specifier, Hash::default());
    let program_ids = message
        .instructions
        .iter()
        .map(|instruction| message.account_keys[usize::from(instruction.program_id_index)])
        .collect::<Vec<_>>();
    assert_eq!(
        program_ids,
        [
            spl_associated_token_account_interface::program::id(),
            *token_program,
            spl_memo_interface::v4::id(),
        ]
    );
    // The transfer doesn't list the new authority, so the memo must, or nothing onchain would
    // require the new authority's approval.
    let memo = message.instructions.last().unwrap();
    assert_eq!(memo.accounts, [0]);
    assert_eq!(memo.data, b"migrate tokens");

    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner],
    )
    .await;
    assert_eq!(token_balance(env, &source).await, 750);
    assert_eq!(token_balance(env, &destination).await, 250);

    // The new authority's token account exists now, and without an amount the rest moves. The
    // emptied source stays open, as asked.
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg, "--no-close"],
    );
    assert_eq!(migration.amount, "7.5");
    assert_eq!(migration.funder, None);
    assert_eq!(migration.rent_recipient, None);
    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(message.instructions.len(), 2);
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner],
    )
    .await;
    assert_eq!(token_balance(env, &source).await, 0);
    assert_eq!(token_balance(env, &destination).await, 1_000);

    // Zero balance left and --no-close, error
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg, "--no-close"],
    );
    assert!(
        stderr.contains(&format!(
            "token account {source} has no balance, remove --no-close to close it"
        )),
        "{stderr}"
    );

    // Without --no-close, we close the empty token account
    let rent_recipient = Address::new_unique();
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &[
            "--owner",
            &owner_arg,
            "--rent-recipient",
            &rent_recipient.to_string(),
        ],
    );
    assert_eq!(migration.amount, "0");
    assert_eq!(migration.funder, None);
    assert_eq!(migration.rent_recipient, Some(rent_recipient.to_string()));
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[owner.pubkey()],
    );
    let message = decode_execution_message(&migration.execution_message);
    let program_ids = message
        .instructions
        .iter()
        .map(|instruction| message.account_keys[usize::from(instruction.program_id_index)])
        .collect::<Vec<_>>();
    assert_eq!(program_ids, [*token_program, spl_memo_interface::v4::id()]);
    let rent = env.rpc.get_balance(&source).await.unwrap();
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner],
    )
    .await;
    assert_closed(env, &source).await;
    assert_eq!(env.rpc.get_balance(&rent_recipient).await.unwrap(), rent);
    assert_eq!(token_balance(env, &destination).await, 1_000);

    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg],
    );
    assert!(
        stderr.contains(&format!("associated token account {source} was not found")),
        "{stderr}"
    );
}

pub async fn migrates_token_balance(env: &TestEnv) {
    migrates_tokens(env, &spl_token_interface::id()).await;
}

pub async fn migrates_token_2022_balance(env: &TestEnv) {
    migrates_tokens(env, &spl_token_2022_interface::id()).await;
}

pub async fn migrates_tokens_with_funder(env: &TestEnv) {
    // The owner holds no SOL, so only the funder can pay rent for the new token account.
    let owner = Keypair::new();
    let funder = Keypair::new();
    let new_owner = Keypair::new();
    let token_program = spl_token_interface::id();
    fund(env, &funder.pubkey()).await;
    let mint = create_mint(env, &token_program, &[]).await;
    let source = mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &[
            "--owner",
            &owner.pubkey().to_string(),
            "--funder",
            &funder.pubkey().to_string(),
        ],
    );
    assert_eq!(migration.funder, Some(funder.pubkey().to_string()));
    // The rent goes to the new authority's programmatic signer by default
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    assert_eq!(
        migration.rent_recipient,
        Some(new_owner_programmatic_signer.to_string())
    );
    let message = decode_execution_message(&migration.execution_message);
    let close = message
        .instructions
        .iter()
        .find(|instruction| {
            message.account_keys[usize::from(instruction.program_id_index)] == token_program
                && instruction.data
                    == close_account(
                        &token_program,
                        &source,
                        &new_owner_programmatic_signer,
                        &owner.pubkey(),
                        &[],
                    )
                    .unwrap()
                    .data
        })
        .expect("the source is closed");
    assert_eq!(
        message.account_keys[usize::from(close.accounts[1])],
        new_owner_programmatic_signer
    );
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[owner.pubkey(), funder.pubkey()],
    );
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner, &funder],
    )
    .await;
    assert_closed(env, &source).await;
    let destination = get_associated_token_address_with_program_id(
        &programmatic_signer(&new_owner.pubkey()),
        &mint,
        &token_program,
    );
    assert_eq!(token_balance(env, &destination).await, 1_000);
}

pub async fn migrates_tokens_between_programmatic_signers(env: &TestEnv) {
    let old_owner = Keypair::new();
    let old_owner_programmatic_signer = programmatic_signer(&old_owner.pubkey());
    let new_owner = Keypair::new();
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    let token_program = spl_token_interface::id();
    // The owner pays rent for the new token account, as the default funder.
    fund(env, &old_owner_programmatic_signer).await;
    let mint = create_mint(env, &token_program, &[]).await;
    let source = mint_tokens(
        env,
        &token_program,
        &mint,
        &old_owner_programmatic_signer,
        1_000,
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // The owner is a programmatic signer, which can't be traced back to its signing pubkey, and
    // nothing signs the relay transaction for it.
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &old_owner_programmatic_signer.to_string()],
    );
    assert_eq!(
        migration.funder,
        Some(old_owner_programmatic_signer.to_string())
    );
    assert_signers(
        &migration.required_signers,
        &[
            authorization(&new_owner.pubkey()),
            Authorization {
                programmatic_signer: old_owner_programmatic_signer.to_string(),
                signer_pubkey: None,
            },
        ],
        &[],
    );

    // Whoever holds the old signing pubkey authorizes its programmatic signer alongside the new one.
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[&old_owner],
        &[],
    )
    .await;
    assert_closed(env, &source).await;
    let destination = get_associated_token_address_with_program_id(
        &new_owner_programmatic_signer,
        &mint,
        &token_program,
    );
    assert_eq!(token_balance(env, &destination).await, 1_000);
}

pub async fn migrates_tokens_with_transfer_fee(env: &TestEnv) {
    let owner = Keypair::new();
    let new_owner = Keypair::new();
    let token_program = spl_token_2022_interface::id();
    fund(env, &owner.pubkey()).await;
    let mint = create_mint(env, &token_program, &[ExtensionType::TransferFeeConfig]).await;
    // The owner receives its tokens through a transfer, so its token account holds withheld fees,
    // which must be harvested before it can close.
    let payer = env.payer.pubkey();
    let payer_account = mint_tokens(env, &token_program, &mint, &payer, 1_000).await;
    let source =
        get_associated_token_address_with_program_id(&owner.pubkey(), &mint, &token_program);
    send(
        env,
        &[
            create_associated_token_account_idempotent(
                &payer,
                &owner.pubkey(),
                &mint,
                &token_program,
            ),
            transfer_checked(
                &token_program,
                &payer_account,
                &mint,
                &source,
                &payer,
                &[],
                1_000,
                DECIMALS,
            )
            .unwrap(),
        ],
        &[],
    )
    .await;
    assert_eq!(token_balance(env, &source).await, 990);
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner.pubkey().to_string()],
    );
    assert_eq!(migration.amount, "9.9");
    assert!(migration.transfer_fee);
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    assert_eq!(
        migration.rent_recipient,
        Some(new_owner_programmatic_signer.to_string())
    );
    let destination = get_associated_token_address_with_program_id(
        &new_owner_programmatic_signer,
        &mint,
        &token_program,
    );
    let message = decode_execution_message(&migration.execution_message);
    let data = message
        .instructions
        .iter()
        .filter(|instruction| {
            message.account_keys[usize::from(instruction.program_id_index)] == token_program
        })
        .map(|instruction| instruction.data.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        data,
        [
            transfer_checked(
                &token_program,
                &source,
                &mint,
                &destination,
                &owner.pubkey(),
                &[],
                990,
                DECIMALS
            )
            .unwrap()
            .data,
            harvest_withheld_tokens_to_mint(&token_program, &mint, &[&source])
                .unwrap()
                .data,
            close_account(
                &token_program,
                &source,
                &new_owner_programmatic_signer,
                &owner.pubkey(),
                &[]
            )
            .unwrap()
            .data,
        ]
    );
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner],
    )
    .await;
    assert_closed(env, &source).await;
    // The fee is withheld in the new authority's token account.
    assert_eq!(token_balance(env, &destination).await, 980);
}

/// An account with a close authority other than its owner needs that authority to close it.
pub async fn migrates_tokens_with_close_authority(env: &TestEnv) {
    let owner = Keypair::new();
    let close_authority = Keypair::new();
    let new_owner = Keypair::new();
    let token_program = spl_token_interface::id();
    fund(env, &owner.pubkey()).await;
    let mint = create_mint(env, &token_program, &[]).await;
    let source = mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    send(
        env,
        &[set_authority(
            &token_program,
            &source,
            Some(&close_authority.pubkey()),
            AuthorityType::CloseAccount,
            &owner.pubkey(),
            &[],
        )
        .unwrap()],
        &[&owner],
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // Moving part of the balance leaves the account open, so the close authority doesn't sign.
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner.pubkey().to_string(), "--amount", "1"],
    );
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[owner.pubkey()],
    );

    // Migrating the remaining balance, and closing, requires the close authority
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner.pubkey().to_string()],
    );
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[owner.pubkey(), close_authority.pubkey()],
    );
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&owner, &close_authority],
    )
    .await;
    assert_closed(env, &source).await;
}

pub async fn rejects_invalid_token_migrations(env: &TestEnv) {
    let owner = Keypair::new();
    let owner_arg = owner.pubkey().to_string();
    let new_owner = Keypair::new();
    let token_program = spl_token_interface::id();
    let mint = create_mint(env, &token_program, &[]).await;
    let source =
        get_associated_token_address_with_program_id(&owner.pubkey(), &mint, &token_program);

    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey()).to_string();
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &new_owner_programmatic_signer],
    );
    assert!(
        stderr.contains(&format!(
            "owner {new_owner_programmatic_signer} is already the new authority's programmatic \
             signer"
        )),
        "{stderr}"
    );

    // An owner that is a programmatic signer can't be the default new authority.
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        None,
        &["--owner", &new_owner_programmatic_signer],
    );
    assert!(
        stderr.contains(&format!(
            "the owner is {new_owner_programmatic_signer}, which is not an on-curve address"
        )),
        "{stderr}"
    );

    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg],
    );
    assert!(
        stderr.contains(&format!("associated token account {source} was not found")),
        "{stderr}"
    );

    mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg, "--amount", "10.01"],
    );
    assert!(
        stderr.contains(&format!(
            "token account {source} holds 10, less than the amount"
        )),
        "{stderr}"
    );
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg, "--amount", "0.001"],
    );
    assert!(
        stderr.contains("amount 0.001 has more than 2 decimal places"),
        "{stderr}"
    );

    // A rent recipient is ignored when the account isn't closed.
    let rent_recipient = Address::new_unique().to_string();
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &[
            "--owner",
            &owner_arg,
            "--amount",
            "1",
            "--rent-recipient",
            &rent_recipient,
        ],
    );
    assert_eq!(migration.rent_recipient, None);
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &[
            "--owner",
            &owner_arg,
            "--no-close",
            "--rent-recipient",
            &rent_recipient,
        ],
    );
    assert!(stderr.contains("cannot be used with"), "{stderr}");

    send(
        env,
        &[freeze_account(&token_program, &source, &mint, &env.payer.pubkey(), &[]).unwrap()],
        &[],
    )
    .await;
    let stderr = migrate_error(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg],
    );
    assert!(
        stderr.contains(&format!("token account {source} is frozen")),
        "{stderr}"
    );
}

/// Token-2022 extensions that would make the transfer fail onchain are rejected up front.
pub async fn rejects_unsupported_token_2022_migrations(env: &TestEnv) {
    let owner = Keypair::new();
    let owner_arg = owner.pubkey().to_string();
    let new_owner = Keypair::new();
    let token_program = spl_token_2022_interface::id();
    let migrate_error = |mint: &Address| {
        migrate_error(
            env,
            "token",
            mint,
            Some(&new_owner.pubkey()),
            &["--owner", &owner_arg],
        )
    };

    // An empty account only needs closing, which the mint can't block.
    let mint = create_mint(env, &token_program, &[ExtensionType::NonTransferable]).await;
    mint_tokens(env, &token_program, &mint, &owner.pubkey(), 0).await;
    let migration: TokenMigrationOutput = migrate(
        env,
        "token",
        &mint,
        Some(&new_owner.pubkey()),
        &["--owner", &owner_arg],
    );
    assert_eq!(migration.amount, "0");
    assert_eq!(
        migration.rent_recipient,
        Some(programmatic_signer(&new_owner.pubkey()).to_string())
    );
    mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    let stderr = migrate_error(&mint);
    assert!(
        stderr.contains(&format!("mint {mint} is non-transferable")),
        "{stderr}"
    );

    let mint = create_mint(env, &token_program, &[ExtensionType::TransferHook]).await;
    mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    let stderr = migrate_error(&mint);
    assert!(
        stderr.contains(&format!("mint {mint} has a transfer hook")),
        "{stderr}"
    );

    // Accounts start frozen, so the owner's is thawed before minting to it, leaving only the new
    // owner's to fail.
    let mint = create_mint(env, &token_program, &[ExtensionType::DefaultAccountState]).await;
    let payer = env.payer.pubkey();
    let source =
        get_associated_token_address_with_program_id(&owner.pubkey(), &mint, &token_program);
    send(
        env,
        &[
            create_associated_token_account_idempotent(
                &payer,
                &owner.pubkey(),
                &mint,
                &token_program,
            ),
            thaw_account(&token_program, &source, &mint, &payer, &[]).unwrap(),
            mint_to_checked(&token_program, &mint, &source, &payer, &[], 1_000, DECIMALS).unwrap(),
        ],
        &[],
    )
    .await;
    let stderr = migrate_error(&mint);
    assert!(
        stderr.contains(&format!("mint {mint} creates token accounts frozen")),
        "{stderr}"
    );

    let mint = create_mint(env, &token_program, &[]).await;
    let source = mint_tokens(env, &token_program, &mint, &owner.pubkey(), 1_000).await;
    send(
        env,
        &[
            reallocate(
                &token_program,
                &source,
                &env.payer.pubkey(),
                &owner.pubkey(),
                &[],
                &[ExtensionType::CpiGuard],
            )
            .unwrap(),
            enable_cpi_guard(&token_program, &source, &owner.pubkey(), &[]).unwrap(),
        ],
        &[&owner],
    )
    .await;
    let stderr = migrate_error(&mint);
    assert!(
        stderr.contains(&format!("token account {source} has the CPI guard enabled")),
        "{stderr}"
    );
}
