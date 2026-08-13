pub(crate) mod address;
pub(crate) mod nonce;
pub(crate) mod transaction;

use {
    crate::{
        cli::{Cli, Command},
        context::CliContext,
    },
    anyhow::Result,
};

pub(crate) fn run(cli: Cli) -> Result<String> {
    let context = CliContext::new(cli.globals);
    match cli.command {
        Command::Address(command) => address::run(command, context.output),
        Command::Nonce(command) => nonce::run(command, &context),
        Command::Transaction(command) => transaction::run(command, &context),
    }
}
