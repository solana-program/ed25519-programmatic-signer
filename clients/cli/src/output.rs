use {
    anyhow::{Context, Result},
    clap::ValueEnum,
    serde::{Deserialize, Serialize},
    solana_native_token::Sol,
    std::fmt::{self, Display},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Display,
    Json,
    JsonCompact,
}

impl OutputFormat {
    pub(crate) fn render(self, output: &(impl Display + Serialize)) -> Result<String> {
        match self {
            Self::Display => Ok(output.to_string()),
            Self::Json => serde_json::to_string_pretty(output).context("failed to encode JSON"),
            Self::JsonCompact => serde_json::to_string(output).context("failed to encode JSON"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceCreateOutput {
    pub signature: String,
    pub nonce_account: String,
    pub authority: String,
    pub nonce: String,
    pub lamports: u64,
}

impl fmt::Display for NonceCreateOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Signature: {}", self.signature)?;
        writeln!(formatter, "Nonce account: {}", self.nonce_account)?;
        writeln!(formatter, "Authority: {}", self.authority)?;
        writeln!(formatter, "Nonce: {}", self.nonce)?;
        write!(formatter, "Balance: {}", Sol(self.lamports))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceShowOutput {
    pub nonce_account: String,
    pub authority: String,
    pub nonce: String,
    pub lamports: u64,
    pub owner: String,
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
