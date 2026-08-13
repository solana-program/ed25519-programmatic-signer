use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        runtime::fs::{read_transaction, write_transaction},
    },
    anyhow::{Result, bail},
    clap::{ArgGroup, Args, ValueHint},
    solana_address::Address,
    solana_hash::Hash,
    spl_programmatic_signer_rust::{inspect, nonce::advance_transaction},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("nonce_account_source")
        .required(true)
        .args(["nonce", "from_transaction"])
))]
pub(crate) struct NonceAdvanceCommand {
    #[arg(
        long,
        value_name = "NONCE_ACCOUNT",
        conflicts_with = "from_transaction"
    )]
    pub(crate) nonce: Option<Address>,
    #[arg(
        long = "from-transaction",
        value_name = "TRANSACTION",
        value_hint = ValueHint::FilePath,
        conflicts_with = "nonce"
    )]
    pub(crate) from_transaction: Option<PathBuf>,
    #[arg(long, value_name = "AUTHORITY")]
    pub(crate) authority: Option<Address>,
    #[arg(long, value_name = "HASH", conflicts_with = "fetch_genesis_hash")]
    pub(crate) genesis_hash: Option<Hash>,
    #[arg(long, conflicts_with = "genesis_hash")]
    pub(crate) fetch_genesis_hash: bool,
    #[arg(
        long,
        value_name = "HASH",
        conflicts_with_all = ["fetch_nonce", "from_transaction"]
    )]
    pub(crate) nonce_value: Option<Hash>,
    #[arg(long, value_name = "OUTFILE", value_hint = ValueHint::FilePath)]
    pub(crate) outfile: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["nonce_value", "from_transaction"])]
    pub(crate) fetch_nonce: bool,
}

pub(super) fn run(command: NonceAdvanceCommand, context: &CliContext) -> Result<String> {
    let source_transaction = match command.from_transaction.as_ref() {
        Some(path) => Some(read_transaction(path)?),
        None => None,
    };
    let source_summary = match source_transaction.as_ref() {
        Some(transaction) => Some(inspect(transaction)?),
        None => None,
    };
    let nonce_account = match (command.nonce, source_summary.as_ref()) {
        (Some(nonce_account), _) => nonce_account,
        (None, Some(summary)) => summary.nonce_account,
        (None, None) => bail!("nonce advance requires --nonce or --from-transaction"),
    };
    let Some(authority) = command.authority else {
        bail!(
            "nonce advance requires --authority for the cold authority backing the \
             ProgrammaticSigner nonce authority"
        );
    };
    let current_nonce = match (command.nonce_value, source_summary.as_ref()) {
        (Some(nonce_value), _) => nonce_value,
        (None, Some(summary)) => *summary.inner_message.recent_blockhash(),
        (None, None) if command.fetch_nonce => {
            let rpc = context.rpc()?;
            decode_nonce_account(&rpc.get_account(&nonce_account)?)?.nonce
        }
        (None, None) => {
            bail!("nonce advance requires --nonce-value, --from-transaction, or --fetch-nonce")
        }
    };
    let genesis_hash = match (command.genesis_hash, source_summary.as_ref()) {
        (Some(genesis_hash), Some(summary)) if genesis_hash != summary.genesis_hash => {
            bail!("--genesis-hash does not match --from-transaction")
        }
        (Some(genesis_hash), _) => genesis_hash,
        (None, Some(summary)) if !command.fetch_genesis_hash => summary.genesis_hash,
        (None, Some(summary)) => {
            let genesis_hash = context.rpc()?.get_genesis_hash()?;
            if genesis_hash != summary.genesis_hash {
                bail!("RPC genesis hash does not match --from-transaction")
            }
            genesis_hash
        }
        (None, _) => context.rpc()?.get_genesis_hash()?,
    };

    let transaction = advance_transaction(nonce_account, authority, current_nonce, genesis_hash)?;
    write_transaction(command.outfile.as_ref(), &transaction)
}
