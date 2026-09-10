use {
    crate::{artifact, client::Client, transaction::WrappedTransaction},
    anyhow::{Result, bail},
    clap::{ArgGroup, Args},
    solana_address::Address,
    solana_message::legacy::Message,
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
    let authority = spl_ed25519_signer_client::ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_client::id(),
        &command.authority,
    );
    let (account, nonce, genesis_hash) = match command.from_transaction {
        Some(path) => {
            let transaction = artifact::read(&path)?;
            if !transaction.inner().signer_keys().contains(&&authority) {
                bail!("authority's PDA is not a required signer of the source file");
            }
            (
                *transaction.nonce_account(),
                transaction.inner().recent_blockhash,
                *transaction.genesis_hash(),
            )
        }
        None => {
            let account = command.nonce.unwrap();
            let state = client.require_nonce(&account).await?;
            if state.authority != authority {
                bail!("nonce is not controlled by this cold authority's PDA");
            }
            (account, state.nonce, client.genesis_hash().await?)
        }
    };
    let transaction = WrappedTransaction::new(
        Message::new_with_blockhash(&[], Some(&authority), &nonce),
        account,
        &[command.authority],
        &[],
        genesis_hash,
    )?;
    artifact::write(command.outfile.as_deref(), &transaction)
}
