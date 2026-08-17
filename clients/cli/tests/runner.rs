use {
    crate::common::{
        helpers::setup_test_env,
        nonce_create::{
            creates_and_shows_nonce_account, creates_nonce_account_with_cold_authority,
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
        async_trial!(creates_and_shows_nonce_account, env, runtime_handle),
        async_trial!(
            creates_nonce_account_with_cold_authority,
            env,
            runtime_handle
        ),
    ];
    libtest_mimic::run(&arguments, tests).exit_code()
}
