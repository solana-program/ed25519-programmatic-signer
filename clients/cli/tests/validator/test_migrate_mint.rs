use {
    crate::{
        common::{execute::programmatic_signer, helpers::TestEnv},
        migration::{
            assert_signers, authorization, create_nonce, decode_execution_message,
            execute_as_new_owner, migrate, migrate_error, migrate_with_stderr, send,
        },
    },
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_system_interface::instruction::create_account,
    spl_programmatic_signer_cli::{Authorization, MintMigrationOutput},
    spl_token_2022_interface::{
        extension::{
            BaseStateWithExtensions, ExtensionType, StateWithExtensions,
            group_pointer::{self, GroupPointer},
            metadata_pointer::{self, MetadataPointer},
            pausable::{self, PausableConfig},
            permanent_delegate::PermanentDelegate,
            transfer_fee::{self, TransferFeeConfig},
        },
        instruction::{
            AuthorityType, initialize_mint_close_authority, initialize_mint2,
            initialize_non_transferable_mint, initialize_permanent_delegate, set_authority,
        },
        state::Mint,
    },
    spl_token_group_interface::{instruction::initialize_group, state::TokenGroup},
    spl_token_metadata_interface::{instruction::initialize, state::TokenMetadata},
};

/// Create a mint. Token-2022 mints get an extension, so decoding has to handle the extended
/// layout most Token-2022 mints use.
pub async fn create_mint(
    env: &TestEnv,
    token_program: &Address,
    mint_authority: &Address,
    freeze_authority: Option<&Address>,
) -> Address {
    let extensions: &[ExtensionType] = if *token_program == spl_token_2022_interface::id() {
        &[ExtensionType::NonTransferable]
    } else {
        &[]
    };
    create_mint_with_extensions(
        env,
        token_program,
        mint_authority,
        freeze_authority,
        extensions,
    )
    .await
}

/// Create a mint with the given extensions, of which only `NonTransferable` is supported.
pub async fn create_mint_with_extensions(
    env: &TestEnv,
    token_program: &Address,
    mint_authority: &Address,
    freeze_authority: Option<&Address>,
    extensions: &[ExtensionType],
) -> Address {
    let mint = Keypair::new();
    let space = ExtensionType::try_calculate_account_len::<Mint>(extensions).unwrap();
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(space)
        .await
        .unwrap();
    let mut instructions = vec![create_account(
        &env.payer.pubkey(),
        &mint.pubkey(),
        lamports,
        space.try_into().unwrap(),
        token_program,
    )];
    for extension in extensions {
        assert_eq!(*extension, ExtensionType::NonTransferable);
        instructions.push(initialize_non_transferable_mint(token_program, &mint.pubkey()).unwrap());
    }
    instructions.push(
        initialize_mint2(
            token_program,
            &mint.pubkey(),
            mint_authority,
            freeze_authority,
            0,
        )
        .unwrap(),
    );
    send(env, &instructions, &[&mint]).await;
    mint.pubkey()
}

/// The mint's mint and freeze authorities.
async fn mint_authorities(env: &TestEnv, mint: &Address) -> (Option<Address>, Option<Address>) {
    let data = env.rpc.get_account_data(mint).await.unwrap();
    let mint = StateWithExtensions::<Mint>::unpack(&data).unwrap().base;
    (mint.mint_authority.into(), mint.freeze_authority.into())
}

async fn migrates_mint_authorities(env: &TestEnv, token_program: &Address) {
    let mint_authority = Keypair::new();
    let freeze_authority = Keypair::new();
    let new_owner = Keypair::new();
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    let mint = create_mint(
        env,
        token_program,
        &mint_authority.pubkey(),
        Some(&freeze_authority.pubkey()),
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    let migration: MintMigrationOutput =
        migrate(env, "mint", &mint, Some(&new_owner.pubkey()), &[]);
    assert_eq!(migration.mint, mint.to_string());
    assert_eq!(migration.token_program, token_program.to_string());
    assert_eq!(migration.authority_types, ["mint", "freeze"]);
    assert!(migration.unset_authority_types.is_empty());
    // Naming both, in any order, builds the same execution message.
    let named: MintMigrationOutput = migrate(
        env,
        "mint",
        &mint,
        Some(&new_owner.pubkey()),
        &["--authority-type", "freeze", "--authority-type", "mint"],
    );
    assert_eq!(named.authority_types, ["mint", "freeze"]);
    assert_eq!(named.execution_message, migration.execution_message);
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
        &[mint_authority.pubkey(), freeze_authority.pubkey()],
    );

    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(message.account_keys[0], new_owner_programmatic_signer);
    assert_eq!(message.lifetime_specifier, Hash::default());
    // `SetAuthority` doesn't list the new authority, so the memo must, or nothing onchain would
    // require the new authority's approval.
    let memo = message.instructions.last().unwrap();
    assert_eq!(
        message.account_keys[usize::from(memo.program_id_index)],
        spl_memo_interface::v4::id()
    );
    assert_eq!(memo.accounts, [0]);
    assert_eq!(
        memo.data,
        format!("migrate mint, freeze authority to {new_owner_programmatic_signer}").as_bytes()
    );

    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&mint_authority, &freeze_authority],
    )
    .await;
    assert_eq!(
        mint_authorities(env, &mint).await,
        (
            Some(new_owner_programmatic_signer),
            Some(new_owner_programmatic_signer)
        )
    );
}

pub async fn migrates_token_mint_authorities(env: &TestEnv) {
    migrates_mint_authorities(env, &spl_token_interface::id()).await;
}

pub async fn migrates_token_2022_mint_authorities(env: &TestEnv) {
    migrates_mint_authorities(env, &spl_token_2022_interface::id()).await;
}

async fn migrates_mint_authorities_one_at_a_time(env: &TestEnv, token_program: &Address) {
    // The owner holds both authorities and moves them to its own programmatic signer, the default
    // new authority.
    let owner = Keypair::new();
    let owner_signer = programmatic_signer(&owner.pubkey());
    let mint = create_mint(env, token_program, &owner.pubkey(), Some(&owner.pubkey())).await;
    let nonce_account = create_nonce(env, &owner.pubkey());

    let (freeze, stderr): (MintMigrationOutput, _) =
        migrate_with_stderr(env, "mint", &mint, None, &["--authority-type", "freeze"]);
    assert_eq!(freeze.authority_types, ["freeze"]);
    // The owner keeps the mint authority, so it's named.
    assert!(
        stderr.contains(&format!(
            "Warning: {} also holds the mint authority of mint {mint}, which this migration \
             doesn't move",
            owner.pubkey()
        )),
        "{stderr}"
    );
    assert_eq!(
        freeze.new_authority_signing_pubkey,
        owner.pubkey().to_string()
    );
    let message = decode_execution_message(&freeze.execution_message);
    assert_eq!(
        message.instructions.last().unwrap().data,
        format!("migrate freeze authority to {owner_signer}").as_bytes()
    );
    assert_signers(
        &freeze.required_signers,
        &[authorization(&owner.pubkey())],
        &[owner.pubkey()],
    );
    execute_as_new_owner(
        env,
        &freeze.execution_message,
        &nonce_account,
        &owner,
        &[],
        &[&owner],
    )
    .await;
    assert_eq!(
        mint_authorities(env, &mint).await,
        (Some(owner.pubkey()), Some(owner_signer))
    );

    let stderr = migrate_error(
        env,
        "mint",
        &mint,
        Some(&owner.pubkey()),
        &["--authority-type", "freeze"],
    );
    assert!(
        stderr.contains(&format!(
            "the freeze authority of mint {mint} is already {owner_signer}"
        )),
        "{stderr}"
    );
    // The freeze authority already moved, so moving both is rejected too.
    let stderr = migrate_error(env, "mint", &mint, Some(&owner.pubkey()), &[]);
    assert!(stderr.contains("is already"), "{stderr}");

    // The freeze authority is now a programmatic signer, which can't be the default new authority.
    let stderr = migrate_error(env, "mint", &mint, None, &["--authority-type", "freeze"]);
    assert!(
        stderr.contains(&format!(
            "the current authority of mint {mint} is {owner_signer}, which is not an on-curve \
             address"
        )),
        "{stderr}"
    );

    // The mint and freeze authorities differ, so neither is the default for moving both.
    let stderr = migrate_error(env, "mint", &mint, None, &[]);
    assert!(
        stderr.contains(&format!(
            "the mint authority {} and freeze authority {owner_signer} of mint {mint} differ",
            owner.pubkey()
        )),
        "{stderr}"
    );

    // The freeze authority already moved, so nothing is left behind.
    let (mint_only, stderr): (MintMigrationOutput, _) =
        migrate_with_stderr(env, "mint", &mint, None, &["--authority-type", "mint"]);
    assert!(!stderr.contains("Warning"), "{stderr}");
    execute_as_new_owner(
        env,
        &mint_only.execution_message,
        &nonce_account,
        &owner,
        &[],
        &[&owner],
    )
    .await;
    assert_eq!(
        mint_authorities(env, &mint).await,
        (Some(owner_signer), Some(owner_signer))
    );
}

pub async fn migrates_token_mint_authorities_one_at_a_time(env: &TestEnv) {
    migrates_mint_authorities_one_at_a_time(env, &spl_token_interface::id()).await;
}

pub async fn migrates_token_2022_mint_authorities_one_at_a_time(env: &TestEnv) {
    migrates_mint_authorities_one_at_a_time(env, &spl_token_2022_interface::id()).await;
}

pub async fn migrates_mint_authorities_between_programmatic_signers(env: &TestEnv) {
    let old_owner = Keypair::new();
    let old_owner_programmatic_signer = programmatic_signer(&old_owner.pubkey());
    let new_owner = Keypair::new();
    let new_owner_programmatic_signer = programmatic_signer(&new_owner.pubkey());
    let mint = create_mint(
        env,
        &spl_token_interface::id(),
        &old_owner_programmatic_signer,
        Some(&old_owner_programmatic_signer),
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // The current authorities are a programmatic signer, which can't be traced back to its signing
    // pubkey, and nothing signs the relay transaction for it.
    let migration: MintMigrationOutput =
        migrate(env, "mint", &mint, Some(&new_owner.pubkey()), &[]);
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
    assert_eq!(
        mint_authorities(env, &mint).await,
        (
            Some(new_owner_programmatic_signer),
            Some(new_owner_programmatic_signer)
        )
    );
}

pub async fn migrates_mint_without_freeze_authority(env: &TestEnv) {
    let mint_authority = Keypair::new();
    let new_owner = Keypair::new();
    let mint = create_mint(
        env,
        &spl_token_interface::id(),
        &mint_authority.pubkey(),
        None,
    )
    .await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // An unset authority is skipped when moving both.
    let migration: MintMigrationOutput =
        migrate(env, "mint", &mint, Some(&new_owner.pubkey()), &[]);
    assert_eq!(migration.authority_types, ["mint"]);
    assert_eq!(migration.unset_authority_types, ["freeze"]);
    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[],
        &[&mint_authority],
    )
    .await;
    assert_eq!(
        mint_authorities(env, &mint).await,
        (Some(programmatic_signer(&new_owner.pubkey())), None)
    );

    // Asking for it explicitly is an error.
    let stderr = migrate_error(
        env,
        "mint",
        &mint,
        Some(&new_owner.pubkey()),
        &["--authority-type", "freeze"],
    );
    assert!(
        stderr.contains(&format!("mint {mint} has no freeze authority")),
        "{stderr}"
    );
    // Also when named alongside the mint authority.
    let stderr = migrate_error(
        env,
        "mint",
        &mint,
        Some(&new_owner.pubkey()),
        &["--authority-type", "mint", "--authority-type", "freeze"],
    );
    assert!(
        stderr.contains(&format!("mint {mint} has no freeze authority")),
        "{stderr}"
    );
}

pub async fn rejects_invalid_mint_migrations(env: &TestEnv) {
    let authority = Keypair::new();
    let owner = Keypair::new();
    let token_program = spl_token_interface::id();
    let mint = create_mint(env, &token_program, &authority.pubkey(), None).await;

    // Passing the programmatic signer in place of its authority.
    let owner_signer = programmatic_signer(&owner.pubkey());
    let stderr = migrate_error(env, "mint", &mint, Some(&owner_signer), &[]);
    assert!(
        stderr.contains(&format!(
            "new authority signing pubkey {owner_signer} is not an on-curve address"
        )),
        "{stderr}"
    );

    let stderr = migrate_error(env, "mint", &env.payer.pubkey(), Some(&owner.pubkey()), &[]);
    assert!(
        stderr.contains(&format!(
            "account {} is owned by 11111111111111111111111111111111, not the Token or Token-2022 \
             Program",
            env.payer.pubkey()
        )),
        "{stderr}"
    );

    // With the mint authority revoked as well, nothing is left to move.
    send(
        env,
        &[set_authority(
            &token_program,
            &mint,
            None,
            AuthorityType::MintTokens,
            &authority.pubkey(),
            &[],
        )
        .unwrap()],
        &[&authority],
    )
    .await;
    let stderr = migrate_error(env, "mint", &mint, Some(&owner.pubkey()), &[]);
    assert!(
        stderr.contains(&format!("mint {mint} has no authorities set")),
        "{stderr}"
    );
}

/// Create a Token-2022 mint whose extension authorities are all `authority`, with metadata and a
/// group stored in the mint. It has no freeze or close authority.
pub async fn create_extension_mint(
    env: &TestEnv,
    mint_authority: &Keypair,
    authority: &Address,
) -> Address {
    let token_program = spl_token_2022_interface::id();
    let mint = Keypair::new();
    let space = ExtensionType::try_calculate_account_len::<Mint>(&[
        ExtensionType::TransferFeeConfig,
        ExtensionType::MintCloseAuthority,
        ExtensionType::PermanentDelegate,
        ExtensionType::Pausable,
        ExtensionType::MetadataPointer,
        ExtensionType::GroupPointer,
    ])
    .unwrap();
    // Initializing the metadata and group grows the mint, so fund the space they take too.
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(space.checked_add(1_000).unwrap())
        .await
        .unwrap();
    let instructions = [
        create_account(
            &env.payer.pubkey(),
            &mint.pubkey(),
            lamports,
            space.try_into().unwrap(),
            &token_program,
        ),
        transfer_fee::instruction::initialize_transfer_fee_config(
            &token_program,
            &mint.pubkey(),
            Some(authority),
            Some(authority),
            10,
            100,
        )
        .unwrap(),
        initialize_mint_close_authority(&token_program, &mint.pubkey(), None).unwrap(),
        initialize_permanent_delegate(&token_program, &mint.pubkey(), authority).unwrap(),
        pausable::instruction::initialize(&token_program, &mint.pubkey(), authority).unwrap(),
        metadata_pointer::instruction::initialize(
            &token_program,
            &mint.pubkey(),
            Some(*authority),
            Some(mint.pubkey()),
        )
        .unwrap(),
        group_pointer::instruction::initialize(
            &token_program,
            &mint.pubkey(),
            Some(*authority),
            Some(mint.pubkey()),
        )
        .unwrap(),
        initialize_mint2(
            &token_program,
            &mint.pubkey(),
            &mint_authority.pubkey(),
            None,
            0,
        )
        .unwrap(),
        initialize(
            &token_program,
            &mint.pubkey(),
            authority,
            &mint.pubkey(),
            &mint_authority.pubkey(),
            "Name".to_string(),
            "SYM".to_string(),
            "https://example.com".to_string(),
        ),
        initialize_group(
            &token_program,
            &mint.pubkey(),
            &mint.pubkey(),
            &mint_authority.pubkey(),
            Some(*authority),
            10,
        ),
    ];
    send(env, &instructions, &[&mint, mint_authority]).await;
    mint.pubkey()
}

/// The extension authorities `create_extension_mint` sets, in the order `migrate mint` lists them.
async fn extension_authorities(env: &TestEnv, mint: &Address) -> Vec<Option<Address>> {
    let data = env.rpc.get_account_data(mint).await.unwrap();
    let state = StateWithExtensions::<Mint>::unpack(&data).unwrap();
    let transfer_fee = state.get_extension::<TransferFeeConfig>().unwrap();
    vec![
        transfer_fee.transfer_fee_config_authority.into(),
        transfer_fee.withdraw_withheld_authority.into(),
        state
            .get_extension::<PermanentDelegate>()
            .unwrap()
            .delegate
            .into(),
        state
            .get_extension::<MetadataPointer>()
            .unwrap()
            .authority
            .into(),
        state
            .get_extension::<GroupPointer>()
            .unwrap()
            .authority
            .into(),
        state
            .get_extension::<PausableConfig>()
            .unwrap()
            .authority
            .into(),
        state
            .get_variable_len_extension::<TokenMetadata>()
            .unwrap()
            .update_authority
            .into(),
        state
            .get_extension::<TokenGroup>()
            .unwrap()
            .update_authority
            .into(),
    ]
}

pub async fn migrates_token_2022_extension_authorities(env: &TestEnv) {
    let owner = Keypair::new();
    let owner_signer = programmatic_signer(&owner.pubkey());
    let mint = create_extension_mint(env, &owner, &owner.pubkey()).await;
    let nonce_account = create_nonce(env, &owner.pubkey());

    // Every set authority moves by default, to the programmatic signer of the shared authority,
    // so none is left behind to warn about.
    let (migration, stderr): (MintMigrationOutput, _) =
        migrate_with_stderr(env, "mint", &mint, None, &[]);
    assert!(!stderr.contains("Warning"), "{stderr}");
    assert_eq!(
        migration.authority_types,
        [
            "mint",
            "transfer-fee-config",
            "withheld-withdraw",
            "permanent-delegate",
            "metadata-pointer",
            "group-pointer",
            "pause",
            "metadata-update",
            "group-update",
        ]
    );
    assert_eq!(migration.unset_authority_types, ["freeze", "close-mint"]);
    assert_signers(
        &migration.required_signers,
        &[authorization(&owner.pubkey())],
        &[owner.pubkey()],
    );
    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(
        message.instructions.last().unwrap().data,
        format!(
            "migrate mint, transfer-fee-config, withheld-withdraw, permanent-delegate, \
             metadata-pointer, group-pointer, pause, metadata-update, group-update authority to \
             {owner_signer}"
        )
        .as_bytes()
    );

    // An unset authority can't be named.
    let stderr = migrate_error(
        env,
        "mint",
        &mint,
        None,
        &["--authority-type", "close-mint"],
    );
    assert!(
        stderr.contains(&format!("mint {mint} has no close-mint authority")),
        "{stderr}"
    );

    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &owner,
        &[],
        &[&owner],
    )
    .await;
    assert_eq!(
        mint_authorities(env, &mint).await,
        (Some(owner_signer), None)
    );
    assert_eq!(
        extension_authorities(env, &mint).await,
        vec![Some(owner_signer); 8]
    );
}

pub async fn migrates_some_token_2022_extension_authorities(env: &TestEnv) {
    let mint_authority = Keypair::new();
    let owner = Keypair::new();
    let new_owner = Keypair::new();
    let new_owner_signer = programmatic_signer(&new_owner.pubkey());
    let mint = create_extension_mint(env, &mint_authority, &owner.pubkey()).await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    // The mint authority differs from the extension authorities, so neither is the default.
    let stderr = migrate_error(env, "mint", &mint, None, &[]);
    assert!(
        stderr.contains(&format!(
            "the mint authority {} and transfer-fee-config authority {} of mint {mint} differ",
            mint_authority.pubkey(),
            owner.pubkey()
        )),
        "{stderr}"
    );

    let (migration, stderr): (MintMigrationOutput, _) = migrate_with_stderr(
        env,
        "mint",
        &mint,
        Some(&new_owner.pubkey()),
        &[
            "--authority-type",
            "pause,permanent-delegate,metadata-update",
        ],
    );
    assert_eq!(
        migration.authority_types,
        ["permanent-delegate", "pause", "metadata-update"]
    );
    // The authorities the owner keeps are named. The mint authority belongs to another address,
    // so it isn't.
    assert!(
        stderr.contains(&format!(
            "Warning: {} also holds the transfer-fee-config, withheld-withdraw, metadata-pointer, \
             group-pointer, group-update authority of mint {mint}, which this migration doesn't \
             move",
            owner.pubkey()
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains(&mint_authority.pubkey().to_string()),
        "{stderr}"
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
    let owner = Some(owner.pubkey());
    let new = Some(new_owner_signer);
    assert_eq!(
        extension_authorities(env, &mint).await,
        [owner, owner, new, owner, owner, new, new, owner]
    );
}

pub async fn rejects_missing_extension_authorities(env: &TestEnv) {
    let authority = Keypair::new();
    let new_owner = Keypair::new();

    let token_mint = create_mint(env, &spl_token_interface::id(), &authority.pubkey(), None).await;
    let stderr = migrate_error(
        env,
        "mint",
        &token_mint,
        Some(&new_owner.pubkey()),
        &["--authority-type", "pause"],
    );
    assert!(
        stderr.contains(&format!(
            "mint {token_mint} is not a Token-2022 mint, so it has no pause authority"
        )),
        "{stderr}"
    );

    // Only has the `NonTransferable` extension.
    let token_2022_mint = create_mint(
        env,
        &spl_token_2022_interface::id(),
        &authority.pubkey(),
        None,
    )
    .await;
    let stderr = migrate_error(
        env,
        "mint",
        &token_2022_mint,
        Some(&new_owner.pubkey()),
        &["--authority-type", "mint,pause"],
    );
    assert!(
        stderr.contains(&format!(
            "mint {token_2022_mint} has no Pausable extension, so it has no pause authority"
        )),
        "{stderr}"
    );
}
