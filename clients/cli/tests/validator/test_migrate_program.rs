use {
    crate::{
        common::{execute::programmatic_signer, helpers::TestEnv},
        migration::{
            assert_signers, authorization, create_nonce, decode_execution_message,
            execute_as_new_owner, migrate, migrate_error, send,
        },
    },
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_loader_v3_interface::{
        get_program_data_address,
        instruction::{
            create_buffer, deploy_with_max_program_len, set_buffer_authority,
            set_upgrade_authority, upgrade, write,
        },
        state::UpgradeableLoaderState,
    },
    solana_message::v1,
    solana_signer::Signer,
    spl_programmatic_signer_cli::ProgramMigrationOutput,
};

/// Program bytes per buffer write, small enough for a legacy transaction.
const WRITE_CHUNK_LEN: usize = 900;

/// A minimal SBPF v3 program, copied from Agave's loader tests. The test validator only deploys
/// v3 programs, and the programs this repository builds are v0.
///
/// Source: <https://github.com/anza-xyz/agave/blob/c4b48df969a9e4f121e14a389bd7bec34c752507/programs/bpf_loader/test_elfs/out/sbpfv3_return_ok.so>
/// (sha256 `7b7f204cc5691a93b8bbc2036b7fdb6977d237711e48c19a65688d53ce216736`).
const PROGRAM: &[u8] = include_bytes!("../fixtures/sbpfv3_return_ok.so");

/// A buffer holding the program, with `authority` as its buffer authority.
async fn write_buffer(env: &TestEnv, program: &[u8], authority: &Keypair) -> Address {
    let buffer = Keypair::new();
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_buffer(
            program.len(),
        ))
        .await
        .unwrap();
    send(
        env,
        &create_buffer(
            &env.payer.pubkey(),
            &buffer.pubkey(),
            &authority.pubkey(),
            lamports,
            program.len(),
        )
        .unwrap(),
        &[&buffer],
    )
    .await;
    for (index, chunk) in program.chunks(WRITE_CHUNK_LEN).enumerate() {
        let offset = index.checked_mul(WRITE_CHUNK_LEN).unwrap();
        send(
            env,
            &[write(
                &buffer.pubkey(),
                &authority.pubkey(),
                offset.try_into().unwrap(),
                chunk.to_vec(),
            )],
            &[authority],
        )
        .await;
    }
    buffer.pubkey()
}

/// Deploy the test program with `upgrade_authority`.
pub async fn deploy_program(env: &TestEnv, upgrade_authority: &Keypair) -> Address {
    let buffer = write_buffer(env, PROGRAM, upgrade_authority).await;
    let program = Keypair::new();
    let lamports = env
        .rpc
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())
        .await
        .unwrap();
    send(
        env,
        &deploy_with_max_program_len(
            &env.payer.pubkey(),
            &program.pubkey(),
            &buffer,
            &upgrade_authority.pubkey(),
            lamports,
            PROGRAM.len(),
            true,
        )
        .unwrap(),
        &[&program, upgrade_authority],
    )
    .await;
    program.pubkey()
}

/// The program data's last-modified slot and upgrade authority.
async fn program_data(env: &TestEnv, program: &Address) -> (u64, Option<Address>) {
    let data = env
        .rpc
        .get_account_data(&get_program_data_address(program))
        .await
        .unwrap();
    match wincode::deserialize(&data).unwrap() {
        UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address,
        } => (slot, upgrade_authority_address),
        state => panic!("unexpected program data state {state:?}"),
    }
}

pub async fn migrates_program_upgrade_authority_to_new_owner(env: &TestEnv) {
    let old_authority = Keypair::new();
    let new_owner = Keypair::new();
    let new_owner_signer = programmatic_signer(&new_owner.pubkey());
    let program = deploy_program(env, &old_authority).await;
    let nonce_account = create_nonce(env, &new_owner.pubkey());

    let migration: ProgramMigrationOutput =
        migrate(env, "program", &program, &new_owner.pubkey(), &[]);
    assert_eq!(migration.program, program.to_string());
    assert_eq!(
        migration.program_data,
        get_program_data_address(&program).to_string()
    );
    assert_eq!(migration.new_owner, new_owner_signer.to_string());
    assert_eq!(
        migration.new_owner_authority,
        new_owner.pubkey().to_string()
    );
    assert_signers(
        &migration.required_signers,
        &[authorization(&new_owner.pubkey())],
        &[old_authority.pubkey()],
    );
    let message = decode_execution_message(&migration.execution_message);
    assert_eq!(message.account_keys[0], new_owner_signer);
    assert_eq!(message.lifetime_specifier, Hash::default());

    execute_as_new_owner(
        env,
        &migration.execution_message,
        &nonce_account,
        &new_owner,
        &[&old_authority],
    )
    .await;
    let (deployed_slot, upgrade_authority) = program_data(env, &program).await;
    assert_eq!(upgrade_authority, Some(new_owner_signer));

    let stderr = migrate_error(env, "program", &program, &new_owner.pubkey(), &[]);
    assert!(
        stderr.contains(&format!(
            "the upgrade authority of program {program} is already {new_owner_signer}"
        )),
        "{stderr}"
    );

    // The programmatic signer can now upgrade. A deployer writes the buffer with their own key,
    // then hands the buffer to the programmatic signer, since an upgrade needs the buffer and
    // program to share an authority.
    let deployer = Keypair::new();
    let buffer = write_buffer(env, PROGRAM, &deployer).await;
    send(
        env,
        &[set_buffer_authority(
            &buffer,
            &deployer.pubkey(),
            &new_owner_signer,
        )],
        &[&deployer],
    )
    .await;
    let spill = Address::new_unique();
    let upgrade = v1::Message::try_compile(
        &new_owner_signer,
        &[upgrade(&program, &buffer, &new_owner_signer, &spill, true)],
        Hash::default(),
    )
    .unwrap();
    execute_as_new_owner(
        env,
        &BASE64_STANDARD.encode(upgrade.serialize()),
        &nonce_account,
        &new_owner,
        &[],
    )
    .await;
    let (upgraded_slot, upgrade_authority) = program_data(env, &program).await;
    assert!(upgraded_slot > deployed_slot);
    assert_eq!(upgrade_authority, Some(new_owner_signer));
    // The buffer's lamports go to the spill account.
    assert!(env.rpc.get_balance(&spill).await.unwrap() > 0);
}

pub async fn rejects_invalid_program_migrations(env: &TestEnv) {
    let authority = Keypair::new();
    let owner = Keypair::new();
    let program = deploy_program(env, &authority).await;

    // Passing the programmatic signer in place of its authority.
    let owner_signer = programmatic_signer(&owner.pubkey());
    let stderr = migrate_error(env, "program", &program, &owner_signer, &[]);
    assert!(
        stderr.contains(&format!(
            "new owner {owner_signer} is not an on-curve address"
        )),
        "{stderr}"
    );

    let stderr = migrate_error(env, "program", &env.payer.pubkey(), &owner.pubkey(), &[]);
    assert!(
        stderr.contains(&format!(
            "account {} is owned by 11111111111111111111111111111111, not the BPF Upgradeable \
             Loader",
            env.payer.pubkey()
        )),
        "{stderr}"
    );

    // Passing the program data in place of the program.
    let program_data = get_program_data_address(&program);
    let stderr = migrate_error(env, "program", &program_data, &owner.pubkey(), &[]);
    assert!(
        stderr.contains(&format!("account {program_data} is not a program")),
        "{stderr}"
    );

    send(
        env,
        &[set_upgrade_authority(&program, &authority.pubkey(), None)],
        &[&authority],
    )
    .await;
    let stderr = migrate_error(env, "program", &program, &owner.pubkey(), &[]);
    assert!(
        stderr.contains(&format!(
            "program {program} is immutable, it has no upgrade authority"
        )),
        "{stderr}"
    );
}
