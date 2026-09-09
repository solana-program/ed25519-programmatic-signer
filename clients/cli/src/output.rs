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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitOutput {
    pub signature: String,
    pub nonce_account: String,
    pub expected_next_nonce: String,
    pub observed_nonce: String,
}

impl fmt::Display for SubmitOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Signature: {}\nNonce account: {}\nExpected next nonce: {}\nObserved nonce: {}",
            self.signature, self.nonce_account, self.expected_next_nonce, self.observed_nonce
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationOutput {
    pub mode: String,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
}

impl fmt::Display for SimulationOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "{} simulation succeeded; compute units: {:?}",
            self.mode, self.units_consumed
        )?;
        if self.mode == "inner" {
            writeln!(
                formatter,
                "Nonce consumption and programmatic signatures were not simulated."
            )?;
        }
        write!(formatter, "{}", self.logs.join("\n"))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutput {
    pub fully_signed: bool,
    pub nonce_account: String,
    pub nonce: String,
    pub genesis_hash: String,
}

impl fmt::Display for VerifyOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Verified nonce {} on {}\nFully signed: {}\nGenesis hash: {}",
            self.nonce, self.nonce_account, self.fully_signed, self.genesis_hash
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextNonceOutput {
    pub nonce_account: String,
    pub next_nonce: String,
}

impl fmt::Display for NextNonceOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.next_nonce.fmt(formatter)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub genesis_hash: String,
    pub signer_program: String,
    pub executor_program: String,
    pub nonce_program: String,
    pub nonce_account: String,
    pub expected_nonce: String,
    pub next_nonce: String,
    pub transaction_signers: Vec<SignerStatus>,
    pub inner_accounts: Vec<Account>,
    pub inner_instructions: Vec<Instruction>,
}

impl fmt::Display for Inspection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "Genesis hash: {}\nSigner program: {}\nExecutor program: {}\nNonce program: {}",
            self.genesis_hash, self.signer_program, self.executor_program, self.nonce_program
        )?;
        writeln!(
            formatter,
            "Nonce account: {}\nExpected nonce: {}\nPredicted next nonce: {}",
            self.nonce_account, self.expected_nonce, self.next_nonce
        )?;
        writeln!(formatter, "Transaction signers:")?;
        for signer in &self.transaction_signers {
            writeln!(
                formatter,
                "  {}: {}",
                signer.address,
                if signer.signed { "signed" } else { "missing" }
            )?;
        }
        writeln!(formatter, "Inner accounts:")?;
        for account in &self.inner_accounts {
            writeln!(
                formatter,
                "  {} signer={} writable={}",
                account.address, account.is_signer, account.is_writable
            )?;
        }
        if self.inner_instructions.is_empty() {
            writeln!(
                formatter,
                "Cancellation: consumes the nonce without executing inner instructions"
            )?;
        }
        for instruction in &self.inner_instructions {
            writeln!(
                formatter,
                "{}\n  Program: {}\n  Accounts: {:?}\n  Data (base64): {}",
                instruction.description,
                instruction.program_id,
                instruction.accounts,
                instruction.data_base64
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SignerStatus {
    pub address: String,
    pub signed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub address: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instruction {
    pub program_id: String,
    pub accounts: Vec<String>,
    pub data_base64: String,
    pub description: String,
}
