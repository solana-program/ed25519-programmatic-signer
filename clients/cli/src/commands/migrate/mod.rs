mod prepare;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::Result,
    clap::{Args, Subcommand},
};

#[derive(Debug, Args)]
pub(crate) struct MigrateCommand {
    #[clap(subcommand)]
    command: MigrateSubcommand,
}

#[derive(Debug, Subcommand)]
enum MigrateSubcommand {
    /// Build unsigned migration transactions for offline signing.
    Prepare(prepare::PrepareCommand),
}

pub(crate) async fn run(
    command: MigrateCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        MigrateSubcommand::Prepare(command) => prepare::run(command, client, output).await,
    }
}
