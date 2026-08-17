use {
    crate::{commands::address::AddressCommand, output::OutputFormat},
    clap::{Parser, Subcommand},
};

#[derive(Debug, Parser)]
#[clap(
    name = "psigner",
    about = "Manage programmatic signer setup and transaction workflows",
    version,
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Format used for command output.
    #[clap(long, global = true, value_enum, default_value_t)]
    pub(crate) output: OutputFormat,

    #[clap(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Derive and print a programmatic signer PDA from an authority address.
    Address(AddressCommand),
}
