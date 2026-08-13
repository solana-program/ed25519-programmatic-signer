mod cli;
mod commands;
mod context;
mod nonce_account;
mod presentation;
mod runtime;

pub use cli::{Cli, OutputFormat};
use {anyhow::Result, clap::Parser};

pub fn run_from_args() -> Result<String> {
    run(Cli::parse())
}

pub fn run(cli: Cli) -> Result<String> {
    commands::run(cli)
}
