use {
    crate::{artifact, client::Client},
    anyhow::{Result, bail},
    clap::{ArgGroup, Args},
    solana_address::Address,
    solana_hash::Hash,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_client::{
        SignOnlyTransaction, inspect, nonce::next_nonce, transaction_from_sign_only_checked,
    },
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[clap(group(ArgGroup::new("snapshot").required(true).args(&["fetch-nonce", "nonce-value", "after"])))]
pub(crate) struct CreateCommand {
    #[clap(long)]
    from_sign_only: PathBuf,
    #[clap(long)]
    nonce: Address,
    /// Cold authorities whose ProgrammaticSigner PDAs authorize the inner message.
    #[clap(long = "authority", required = true, multiple_occurrences = true)]
    authorities: Vec<Address>,
    /// Live signer required by both the wrapped message and outer relay.
    #[clap(long = "submit-signer", multiple_occurrences = true)]
    submit_signers: Vec<Address>,
    #[clap(long)]
    fetch_nonce: bool,
    #[clap(long)]
    nonce_value: Option<Hash>,
    /// Offline snapshot authority; defaults to the first cold authority's PDA.
    #[clap(long, conflicts_with = "fetch-nonce")]
    nonce_authority: Option<Address>,
    /// Build a successor using the predicted nonce and cluster from this predecessor file.
    #[clap(long)]
    after: Option<PathBuf>,
    /// Required for offline creation unless --after supplies the cluster.
    #[clap(long)]
    genesis_hash: Option<Hash>,
    #[clap(long)]
    outfile: Option<PathBuf>,
}

pub(super) async fn run(command: CreateCommand, client: &Client) -> Result<String> {
    let source = SignOnlyTransaction::from_json(&artifact::read_text(&command.from_sign_only)?)?;
    let predecessor = command.after.as_deref().map(artifact::read).transpose()?;
    let predecessor_summary = predecessor.as_ref().map(inspect).transpose()?;
    if let Some(summary) = &predecessor_summary {
        if summary.nonce_account != command.nonce {
            bail!("predecessor uses a different nonce account");
        }
        if command
            .genesis_hash
            .is_some_and(|hash| hash != summary.genesis_hash)
        {
            bail!("genesis hash does not match predecessor");
        }
    }
    let genesis_hash = match (command.genesis_hash, &predecessor_summary) {
        (Some(hash), _) => hash,
        (None, Some(summary)) => summary.genesis_hash,
        (None, None) if command.fetch_nonce => client.genesis_hash().await?,
        _ => bail!("--genesis-hash is required for offline creation"),
    };
    let state = if command.fetch_nonce {
        let state = client.require_nonce(&command.nonce).await?;
        if genesis_hash != client.genesis_hash().await? {
            bail!("genesis hash does not match RPC cluster");
        }
        state
    } else {
        let nonce = match (&predecessor, command.nonce_value) {
            (Some(transaction), _) => next_nonce(transaction)?,
            (None, Some(nonce)) => nonce,
            _ => unreachable!("clap requires a nonce source"),
        };
        Nonce {
            nonce,
            authority: command.nonce_authority.unwrap_or_else(|| {
                ProgrammaticSigner::derive_address(
                    &spl_ed25519_signer_client::id(),
                    &command.authorities[0],
                )
            }),
        }
    };
    let transaction = transaction_from_sign_only_checked(
        &source,
        command.nonce,
        &state,
        &command.authorities,
        &command.submit_signers,
        genesis_hash,
    )?;
    artifact::write(command.outfile.as_deref(), &transaction)
}
