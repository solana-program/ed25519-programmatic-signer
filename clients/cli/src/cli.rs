use {
    crate::{
        commands::{address::AddressCommand, nonce::NonceCommand},
        output::OutputFormat,
    },
    clap::{Args, Parser, Subcommand, ValueHint},
    solana_clap_v3_utils::input_parsers::parse_url_or_moniker,
    solana_commitment_config::CommitmentConfig,
    std::path::PathBuf,
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

    #[clap(flatten)]
    pub(crate) client: ClientArgs,

    #[clap(subcommand)]
    pub(crate) command: Command,
}

/// Global options consumed by [`crate::client::Client`]. Each one overrides a value the Solana
/// CLI configuration file otherwise supplies.
#[derive(Debug, Args)]
pub(crate) struct ClientArgs {
    /// Solana CLI configuration file. Uses the standard path when omitted.
    #[clap(
        short = 'C',
        long,
        global = true,
        value_hint = ValueHint::FilePath
    )]
    pub(crate) config: Option<PathBuf>,

    /// Solana RPC URL or cluster moniker. Full monikers and their first letters are supported.
    #[clap(
        short = 'u',
        long,
        global = true,
        value_parser = parse_url_or_moniker
    )]
    pub(crate) url: Option<String>,

    /// Commitment level used for RPC queries and transaction confirmation.
    /// Uses Solana CLI config when omitted.
    #[clap(long, global = true)]
    pub(crate) commitment: Option<CommitmentConfig>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Derive and print a programmatic signer PDA from an authority address.
    Address(AddressCommand),
    /// Manage SPL Nonce accounts used by programmatic signer transactions.
    Nonce(NonceCommand),
}
