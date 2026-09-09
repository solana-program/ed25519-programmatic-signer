use {
    crate::{
        client::Client,
        output::{NonceShowOutput, OutputFormat},
    },
    anyhow::{Result, anyhow},
    clap::Args,
    solana_address::Address,
};

#[derive(Debug, Args)]
pub(crate) struct ShowCommand {
    /// Address of the SPL Nonce account to inspect.
    pub(crate) nonce_account: Address,
}

pub(super) async fn run(
    command: ShowCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let account = client
        .nonce_account(&command.nonce_account)
        .await?
        .ok_or_else(|| anyhow!("account {} was not found", command.nonce_account))?;

    output.render(&NonceShowOutput {
        nonce_account: command.nonce_account.to_string(),
        authority: account.state.authority.to_string(),
        nonce: account.state.nonce.to_string(),
        lamports: account.lamports,
        owner: spl_nonce_interface::id().to_string(),
    })
}
