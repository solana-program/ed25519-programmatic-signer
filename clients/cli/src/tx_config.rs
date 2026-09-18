use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct TransactionConfigArgs {
    /// Maximum compute units. Uses the command's default when omitted.
    #[clap(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) compute_unit_limit: Option<u32>,
    /// Total priority fee in lamports, paid in addition to the base transaction fee.
    #[clap(long)]
    pub(crate) priority_fee: Option<u64>,
}
