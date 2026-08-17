pub(crate) mod address;

use {
    crate::cli::{Cli, Command},
    anyhow::Result,
};

pub(crate) fn run(cli: Cli) -> Result<String> {
    match cli.command {
        Command::Address(command) => address::run(command, cli.output),
    }
}
