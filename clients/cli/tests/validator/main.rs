use {
    self::{
        common::helpers::setup_test_env,
        nonce_create::{
            creates_and_shows_nonce_account, creates_nonce_account_with_cold_authority,
            creates_nonce_account_with_generated_keypair,
        },
        test_tx_submit_execution::{
            rejects_invalid_nonce_accounts_before_wallet_loading,
            rejects_nonce_authority_outside_inner_signers,
            submits_approved_transfer_and_rejects_replay,
            submits_with_file_and_cli_signatures_in_authority_order,
        },
    },
    libtest_mimic::{Arguments, Trial},
    std::{process::ExitCode, sync::Arc},
};

#[path = "../common/mod.rs"]
pub mod common;
mod nonce_create;
mod test_tx_submit_execution;

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
        async_trial!(
            rejects_invalid_nonce_accounts_before_wallet_loading,
            env,
            runtime_handle
        ),
        async_trial!(
            rejects_nonce_authority_outside_inner_signers,
            env,
            runtime_handle
        ),
        async_trial!(
            submits_approved_transfer_and_rejects_replay,
            env,
            runtime_handle
        ),
        async_trial!(
            submits_with_file_and_cli_signatures_in_authority_order,
            env,
            runtime_handle
        ),
    ];
    libtest_mimic::run(&arguments, tests).exit_code()
}
