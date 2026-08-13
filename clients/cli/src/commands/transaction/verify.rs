use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        presentation::transaction::{
            NonceCheckOutput, SignerStatusOutput, VerifyOutput, render_verify,
        },
        runtime::fs::read_transaction,
    },
    anyhow::{Result, bail},
    clap::{ArgGroup, Args, ValueHint},
    solana_address::Address,
    solana_hash::Hash,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::{inspect, is_fully_signed, verify::verify},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("nonce_snapshot")
        .required(true)
        .args(["fetch_nonce", "nonce_value"])
))]
pub(crate) struct TransactionVerifyCommand {
    #[arg(value_name = "TRANSACTION", value_hint = ValueHint::FilePath)]
    pub(crate) transaction: PathBuf,
    #[arg(long, conflicts_with = "nonce_value")]
    pub(crate) fetch_nonce: bool,
    #[arg(long, value_name = "HASH", requires = "nonce_authority")]
    pub(crate) nonce_value: Option<Hash>,
    #[arg(long, value_name = "ADDRESS")]
    pub(crate) nonce_authority: Option<Address>,
    #[arg(long, value_name = "HASH", conflicts_with = "fetch_genesis_hash")]
    pub(crate) genesis_hash: Option<Hash>,
    #[arg(long, conflicts_with = "genesis_hash")]
    pub(crate) fetch_genesis_hash: bool,
}

pub(super) fn run(command: TransactionVerifyCommand, context: &CliContext) -> Result<String> {
    let transaction = read_transaction(&command.transaction)?;
    let summary = inspect(&transaction)?;
    let needs_rpc =
        command.fetch_nonce || command.fetch_genesis_hash || command.genesis_hash.is_none();
    let rpc = if needs_rpc {
        Some(context.rpc()?)
    } else {
        None
    };
    let genesis_hash = match command.genesis_hash {
        Some(genesis_hash) => genesis_hash,
        None => rpc.as_ref().unwrap().get_genesis_hash()?,
    };
    let nonce_check = if command.fetch_nonce {
        let nonce =
            decode_nonce_account(&rpc.as_ref().unwrap().get_account(&summary.nonce_account)?)?;
        verify(&transaction, &nonce, &summary.nonce_account, &genesis_hash)?;
        Some(NonceCheckOutput {
            source: String::from("rpc"),
            nonce: nonce.nonce.to_string(),
            authority: nonce.authority.to_string(),
        })
    } else if let Some(nonce_value) = command.nonce_value {
        let Some(nonce_authority) = command.nonce_authority else {
            bail!("transaction verify --nonce-value requires --nonce-authority");
        };
        let nonce = Nonce {
            nonce: nonce_value,
            authority: nonce_authority,
        };
        verify(&transaction, &nonce, &summary.nonce_account, &genesis_hash)?;
        Some(NonceCheckOutput {
            source: String::from("args"),
            nonce: nonce.nonce.to_string(),
            authority: nonce.authority.to_string(),
        })
    } else {
        bail!("transaction verify requires --fetch-nonce or --nonce-value with --nonce-authority");
    };
    render_verify(
        context.output,
        VerifyOutput {
            fully_signed: is_fully_signed(&transaction),
            transaction_signers: summary
                .wrapper_signers
                .iter()
                .map(|status| SignerStatusOutput {
                    address: status.address.to_string(),
                    signed: status.signed,
                })
                .collect(),
            genesis_hash: genesis_hash.to_string(),
            nonce_account: summary.nonce_account.to_string(),
            nonce_check,
        },
    )
}
