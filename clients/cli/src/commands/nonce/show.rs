use {
    crate::{client::Client, output::OutputFormat},
    anyhow::{Result, anyhow},
    clap::Args,
    serde::Serialize,
    solana_address::Address,
    solana_native_token::Sol,
    std::fmt,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NonceShowOutput {
    nonce_account: String,
    authority: String,
    nonce: String,
    lamports: u64,
    owner: String,
}

impl fmt::Display for NonceShowOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Nonce account: {}", self.nonce_account)?;
        writeln!(formatter, "Authority: {}", self.authority)?;
        writeln!(formatter, "Nonce: {}", self.nonce)?;
        writeln!(formatter, "Balance: {}", Sol(self.lamports))?;
        write!(formatter, "Owner: {}", self.owner)
    }
}
