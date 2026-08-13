use {
    crate::commands::{
        address::AddressCommand, nonce::NonceCommand, transaction::TransactionCommand,
    },
    clap::{Args, Parser, Subcommand, ValueEnum},
};

#[derive(Debug, Parser)]
#[command(name = "psigner")]
#[command(about = "Create, inspect, sign, and submit programmatic signer transactions")]
#[command(version, propagate_version = true)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    #[command(flatten)]
    pub(crate) globals: GlobalArgs,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct GlobalArgs {
    #[arg(
        short = 'u',
        long,
        global = true,
        value_name = "URL_OR_MONIKER",
        help = "RPC URL or moniker: localhost, localnet, mainnet-beta, devnet, testnet"
    )]
    pub(crate) url: Option<String>,

    #[arg(
        short = 'o',
        long,
        global = true,
        value_enum,
        default_value_t = OutputFormat::Display,
        help = "Display format for informational command output"
    )]
    pub(crate) output: OutputFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Display,
    Json,
    JsonCompact,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Derive and print the ProgrammaticSigner PDA for a cold authority address.
    Address(AddressCommand),
    /// Create, show, or advance SPL Nonce accounts used by cold-signed transactions.
    Nonce(NonceCommand),
    /// Create, inspect, sign, merge, verify, simulate, or submit cold-signed transactions.
    Transaction(TransactionCommand),
}
