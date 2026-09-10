use {
    crate::{
        artifact,
        client::Client,
        output::{OutputFormat, VerifyOutput},
    },
    anyhow::{Result, bail},
    clap::{ArgGroup, Args},
    solana_address::Address,
    solana_hash::Hash,
    spl_nonce_interface::state::Nonce,
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[clap(group(ArgGroup::new("snapshot").required(true).args(&["fetch-nonce", "nonce-value"])))]
pub(crate) struct VerifyCommand {
    transaction: PathBuf,
    #[clap(long)]
    fetch_nonce: bool,
    #[clap(long, requires = "nonce-authority")]
    nonce_value: Option<Hash>,
    #[clap(long, requires = "nonce-value")]
    nonce_authority: Option<Address>,
    /// Required with an offline snapshot; checked against RPC when --fetch-nonce is used.
    #[clap(long)]
    genesis_hash: Option<Hash>,
    /// Allow missing signature slots while checking a partially signed file.
    #[clap(long)]
    allow_partial: bool,
}

pub(super) async fn run(
    command: VerifyCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let transaction = artifact::read(&command.transaction)?;
    let (state, genesis_hash) = if command.fetch_nonce {
        let genesis_hash = client.genesis_hash().await?;
        if command
            .genesis_hash
            .is_some_and(|hash| hash != genesis_hash)
        {
            bail!("genesis hash does not match RPC cluster");
        }
        (
            client.require_nonce(transaction.nonce_account()).await?,
            genesis_hash,
        )
    } else {
        let Some(genesis_hash) = command.genesis_hash else {
            bail!("--genesis-hash is required for offline verification");
        };
        (
            Nonce {
                nonce: command.nonce_value.unwrap(),
                authority: command.nonce_authority.unwrap(),
            },
            genesis_hash,
        )
    };
    transaction.verify(&state, &genesis_hash)?;
    let fully_signed = transaction.is_fully_signed();
    if !fully_signed && !command.allow_partial {
        bail!("transaction is not fully signed; use --allow-partial to inspect partial progress");
    }
    output.render(&VerifyOutput {
        fully_signed,
        nonce_account: transaction.nonce_account().to_string(),
        nonce: state.nonce.to_string(),
        genesis_hash: genesis_hash.to_string(),
    })
}
