pub(crate) mod address;
pub(crate) mod nonce;
pub(crate) mod transaction;

use {
    crate::{
        cli::{Cli, Command},
        client::Client,
    },
    anyhow::Result,
    clap::ArgMatches,
};

pub(crate) async fn run(cli: Cli, matches: ArgMatches) -> Result<String> {
    match cli.command {
        Command::Address(command) => address::run(command, cli.output),
        Command::Transaction(command) => {
            let client = Client::new(cli.client, matches)?;
            transaction::run(command, &client, cli.output).await
        }
        Command::Nonce(command) => {
            let client = Client::new(cli.client, matches)?;
            nonce::run(command, &client, cli.output).await
        }
    }
}
