use {
    crate::{artifact, client::Client},
    anyhow::{Result, bail},
    clap::{ArgGroup, Args},
    solana_address::Address,
    solana_hash::Hash,
    spl_programmatic_signer_client::{inspect, nonce::advance_transaction},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[clap(group(ArgGroup::new("source").required(true).args(&["from-transaction", "nonce"])))]
pub(crate) struct AdvanceCommand {
    /// Cancel the nonce used by this file. Builds an offline cancellation for cold signing.
    #[clap(long)]
    from_transaction: Option<PathBuf>,
    /// Fetch this account's current nonce to build a cancellation.
    #[clap(long)]
    nonce: Option<Address>,
    /// Cold authority backing the nonce's ProgrammaticSigner PDA.
    #[clap(long)]
    authority: Address,
    #[clap(long)]
    outfile: Option<PathBuf>,
}

pub(super) async fn run(command: AdvanceCommand, client: &Client) -> Result<String> {
    let (account, nonce, genesis_hash) = match command.from_transaction {
        Some(path) => {
            let summary = inspect(&artifact::read(&path)?)?;
            let authority = spl_ed25519_signer_client::ProgrammaticSigner::derive_address(
                &spl_ed25519_signer_client::id(),
                &command.authority,
            );
            if !summary.inner_required_signers.contains(&authority) {
                bail!("authority's PDA is not a required signer of the source file");
            }
            (
                summary.nonce_account,
                Hash::new_from_array(summary.inner_message.recent_blockhash().to_bytes()),
                summary.genesis_hash,
            )
        }
        None => {
            let account = command.nonce.unwrap();
            let state = client.require_nonce(&account).await?;
            let authority = spl_ed25519_signer_client::ProgrammaticSigner::derive_address(
                &spl_ed25519_signer_client::id(),
                &command.authority,
            );
            if state.authority != authority {
                bail!("nonce is not controlled by this cold authority's PDA");
            }
            (account, state.nonce, client.genesis_hash().await?)
        }
    };
    artifact::write(
        command.outfile.as_deref(),
        &advance_transaction(account, command.authority, nonce, genesis_hash)?,
    )
}
