use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        presentation::transaction::{SubmitOutput, render_submit},
        runtime::{
            config::keypair_source,
            fs::{read_transaction, write_submit_transaction},
            signer::{read_signer, read_signers, signer_refs},
        },
    },
    anyhow::{Result, bail},
    clap::{Args, ValueHint},
    solana_hash::Hash,
    spl_programmatic_signer_rust::{
        inspect,
        submit::submit_transaction,
        verify::{verify, verify_genesis_hash},
    },
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionSubmitCommand {
    #[arg(value_name = "TRANSACTION", value_hint = ValueHint::FilePath)]
    pub(crate) transaction: PathBuf,
    #[arg(long, value_name = "KEYPAIR_OR_URL", help = "Fee payer signer source")]
    pub(crate) fee_payer: Option<String>,
    #[arg(
        long = "submit-signer",
        value_name = "KEYPAIR_OR_URL",
        help = "Required live submit signer source for designated-relayer files"
    )]
    pub(crate) submit_signers: Vec<String>,
    #[arg(long, value_name = "HASH")]
    pub(crate) blockhash: Option<Hash>,
    #[arg(long, value_name = "HASH", conflicts_with = "fetch_genesis_hash")]
    pub(crate) genesis_hash: Option<Hash>,
    #[arg(long, conflicts_with = "genesis_hash")]
    pub(crate) fetch_genesis_hash: bool,
    #[arg(long, conflicts_with = "no_send")]
    pub(crate) skip_preflight: bool,
    #[arg(long, conflicts_with = "no_send")]
    pub(crate) skip_verify: bool,
    #[arg(long)]
    pub(crate) no_send: bool,
    #[arg(
        long,
        value_name = "OUTFILE",
        value_hint = ValueHint::FilePath,
        requires = "no_send"
    )]
    pub(crate) outfile: Option<PathBuf>,
}

pub(super) fn run(command: TransactionSubmitCommand, context: &CliContext) -> Result<String> {
    let wrapped_transaction = read_transaction(&command.transaction)?;
    let fee_payer_source = keypair_source(command.fee_payer.as_deref())?;
    let mut wallet_manager = None;
    let fee_payer = read_signer(&fee_payer_source, "fee-payer", &mut wallet_manager)?;
    let submit_signers = read_signers(
        &command.submit_signers,
        "submit-signer",
        &mut wallet_manager,
    )?;
    let submit_signer_refs = signer_refs(&submit_signers);
    let needs_rpc = command.blockhash.is_none() || !command.no_send || command.fetch_genesis_hash;
    let rpc = if needs_rpc {
        Some(context.rpc()?)
    } else {
        None
    };
    let summary = inspect(&wrapped_transaction)?;
    if command.no_send {
        if command.genesis_hash.is_some() || command.fetch_genesis_hash {
            let genesis_hash = match command.genesis_hash {
                Some(genesis_hash) => genesis_hash,
                None => rpc
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("RPC client is required to fetch genesis hash"))?
                    .get_genesis_hash()?,
            };
            verify_genesis_hash(&wrapped_transaction, &genesis_hash)?;
        }
    } else if !command.skip_verify {
        let Some(rpc) = rpc.as_ref() else {
            bail!("RPC client is required before submit");
        };
        let nonce = decode_nonce_account(&rpc.get_account(&summary.nonce_account)?)?;
        let genesis_hash = match command.genesis_hash {
            Some(genesis_hash) => genesis_hash,
            None => rpc.get_genesis_hash()?,
        };
        verify(
            &wrapped_transaction,
            &nonce,
            &summary.nonce_account,
            &genesis_hash,
        )?;
    }
    let blockhash = match (command.blockhash, rpc.as_ref()) {
        (Some(blockhash), _) => blockhash,
        (None, Some(rpc)) => rpc.get_latest_blockhash()?,
        (None, None) => bail!("--blockhash is required when building without RPC"),
    };
    let transaction = submit_transaction(
        &wrapped_transaction,
        fee_payer.as_ref(),
        &submit_signer_refs,
        blockhash,
    )?;
    if command.no_send {
        return write_submit_transaction(command.outfile.as_ref(), &transaction);
    }

    let Some(rpc) = rpc.as_ref() else {
        bail!("RPC client is required to submit a transaction");
    };
    let submitted_nonce = *summary.inner_message.recent_blockhash();
    let signature = rpc.send_transaction(&transaction, command.skip_preflight)?;
    rpc.confirm_signature(&signature)?;
    let account = rpc.get_account_until(
        &summary.nonce_account,
        |account| Ok(decode_nonce_account(account)?.nonce != submitted_nonce),
        "advanced past the submitted nonce",
    )?;
    let nonce = decode_nonce_account(&account)?;
    render_submit(
        context.output,
        SubmitOutput {
            signature,
            nonce_account: summary.nonce_account.to_string(),
            advanced_nonce: nonce.nonce.to_string(),
        },
    )
}
