use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        runtime::{
            fs::{read_text, write_transaction},
            signer::programmatic_signer,
        },
    },
    anyhow::{Result, bail},
    clap::{Args, ValueHint},
    solana_address::Address,
    solana_hash::Hash,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::{
        SignOnlyTransaction, transaction_from_sign_only, transaction_from_sign_only_checked,
    },
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionCreateCommand {
    #[arg(
        long,
        value_name = "SIGN_ONLY_JSON",
        value_hint = ValueHint::FilePath
    )]
    pub(crate) from_sign_only: PathBuf,
    #[arg(long, value_name = "NONCE_ACCOUNT")]
    pub(crate) nonce: Address,
    #[arg(long = "authority", value_name = "AUTHORITY", num_args = 1.., required = true)]
    pub(crate) authorities: Vec<Address>,
    #[arg(long = "submit-signer", value_name = "ADDRESS")]
    pub(crate) submit_signers: Vec<Address>,
    #[arg(long, value_name = "HASH", conflicts_with = "fetch_genesis_hash")]
    pub(crate) genesis_hash: Option<Hash>,
    #[arg(long, conflicts_with = "genesis_hash")]
    pub(crate) fetch_genesis_hash: bool,
    #[arg(long, value_name = "HASH", conflicts_with = "fetch_nonce")]
    pub(crate) nonce_value: Option<Hash>,
    #[arg(long, value_name = "ADDRESS", requires = "nonce_value")]
    pub(crate) nonce_authority: Option<Address>,
    #[arg(long, conflicts_with = "nonce_value")]
    pub(crate) fetch_nonce: bool,
    #[arg(long, value_name = "OUTFILE", value_hint = ValueHint::FilePath)]
    pub(crate) outfile: Option<PathBuf>,
}

pub(super) fn run(command: TransactionCreateCommand, context: &CliContext) -> Result<String> {
    if command.authorities.is_empty() {
        bail!("transaction create requires at least one --authority");
    }
    let sign_only_json = read_text(&command.from_sign_only)?;
    let sign_only = SignOnlyTransaction::from_json(&sign_only_json)?;
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
    let transaction = match (command.fetch_nonce, command.nonce_value) {
        (true, _) => {
            let nonce = decode_nonce_account(&rpc.as_ref().unwrap().get_account(&command.nonce)?)?;
            transaction_from_sign_only_checked(
                &sign_only,
                command.nonce,
                &nonce,
                &command.authorities,
                &command.submit_signers,
                genesis_hash,
            )?
        }
        (false, Some(nonce_value)) => {
            let nonce_authority = command
                .nonce_authority
                .unwrap_or_else(|| programmatic_signer(&command.authorities[0]));
            let nonce = Nonce {
                nonce: nonce_value,
                authority: nonce_authority,
            };
            transaction_from_sign_only_checked(
                &sign_only,
                command.nonce,
                &nonce,
                &command.authorities,
                &command.submit_signers,
                genesis_hash,
            )?
        }
        (false, None) => transaction_from_sign_only(
            &sign_only,
            command.nonce,
            &command.authorities,
            &command.submit_signers,
            genesis_hash,
        )?,
    };
    write_transaction(command.outfile.as_ref(), &transaction)
}
