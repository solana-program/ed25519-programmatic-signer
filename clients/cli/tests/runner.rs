use {
    crate::common::{
        helpers::setup_test_env,
        nonce_create::{
            creates_and_shows_nonce_account, creates_nonce_account_with_cold_authority,
            creates_nonce_account_with_generated_keypair,
        },
        transactions::{
            cancellation_invalidates_pending_file,
            designated_relayer_merges_signatures_and_requires_live_signer,
            failed_inner_simulation_and_submission_preserve_nonce, precomputed_chain_runs_in_order,
            token_transfer_is_decoded_and_lands, transfer_offline_signing_simulation_and_replay,
        },
    },
    libtest_mimic::{Arguments, Trial},
    std::{process::ExitCode, sync::Arc},
};

mod common;

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
        async_trial!(token_transfer_is_decoded_and_lands, env, runtime_handle),
        async_trial!(
            transfer_offline_signing_simulation_and_replay,
            env,
            runtime_handle
        ),
        async_trial!(cancellation_invalidates_pending_file, env, runtime_handle),
        async_trial!(precomputed_chain_runs_in_order, env, runtime_handle),
        async_trial!(
            designated_relayer_merges_signatures_and_requires_live_signer,
            env,
            runtime_handle
        ),
        async_trial!(
            failed_inner_simulation_and_submission_preserve_nonce,
            env,
            runtime_handle
        ),
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
