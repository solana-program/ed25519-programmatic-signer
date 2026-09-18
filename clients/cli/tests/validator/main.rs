use {
    self::{
        common::helpers::setup_test_env,
        nonce_create::{
            creates_and_shows_nonce_account, creates_nonce_account_with_cold_authority,
            creates_nonce_account_with_generated_keypair,
        },
        test_migrate_stake_prepare::{
            prepared_migration_executes_with_sdk_signatures, prepares_and_executes_stake_lockups,
            prepares_selected_stake_authorities, rejects_invalid_migration_inputs,
            resolves_migration_fee_payer, writes_handoff_artifact_and_receipt,
        },
    },
    libtest_mimic::{Arguments, Trial},
    std::{process::ExitCode, sync::Arc},
};

#[path = "../common/mod.rs"]
mod common;
mod nonce_create;
mod test_migrate_stake_prepare;

macro_rules! async_trial {
    ($test:ident, $env:ident, $runtime:ident) => {{
        let test_env = Arc::clone(&$env);
        let handle = $runtime.clone();
        Trial::test(stringify!($test), move || {
            handle.block_on($test(&test_env));
            Ok(())
        })
    }};
}

fn main() -> ExitCode {
    let arguments = Arguments::from_args();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let env = Arc::new(runtime.block_on(setup_test_env()));
    let runtime_handle = runtime.handle().clone();
    let tests = vec![
        async_trial!(prepares_selected_stake_authorities, env, runtime_handle),
        async_trial!(writes_handoff_artifact_and_receipt, env, runtime_handle),
        async_trial!(resolves_migration_fee_payer, env, runtime_handle),
        async_trial!(rejects_invalid_migration_inputs, env, runtime_handle),
        async_trial!(
            prepared_migration_executes_with_sdk_signatures,
            env,
            runtime_handle
        ),
        async_trial!(prepares_and_executes_stake_lockups, env, runtime_handle),
        async_trial!(creates_and_shows_nonce_account, env, runtime_handle),
        async_trial!(
            creates_nonce_account_with_generated_keypair,
            env,
            runtime_handle
        ),
        async_trial!(
            creates_nonce_account_with_cold_authority,
            env,
            runtime_handle
        ),
    ];
    libtest_mimic::run(&arguments, tests).exit_code()
}
