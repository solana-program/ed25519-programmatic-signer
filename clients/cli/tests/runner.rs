use {
    self::test_tx_submit_validator::{
        rejects_invalid_nonce_accounts_before_wallet_loading,
        rejects_nonce_authority_outside_inner_signers, submits_approved_transfer_once,
        submits_with_file_and_cli_signatures,
    },
    crate::common::{
        helpers::setup_test_env,
        nonce_create::{
            creates_and_shows_nonce_account, creates_nonce_account_with_cold_authority,
            creates_nonce_account_with_generated_keypair,
        },
    },
    libtest_mimic::{Arguments, Trial},
    std::{process::ExitCode, sync::Arc},
};

pub mod common;
mod test_tx_submit_validator;

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
        async_trial!(submits_approved_transfer_once, env, runtime_handle),
        async_trial!(submits_with_file_and_cli_signatures, env, runtime_handle),
    ];
    libtest_mimic::run(&arguments, tests).exit_code()
}
