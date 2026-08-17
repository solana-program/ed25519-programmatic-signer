use {
    crate::cli::ClientArgs,
    anyhow::{Context, Result, anyhow, bail},
    solana_account::Account,
    solana_address::Address,
    solana_clap_v3_utils::input_parsers::parse_url_or_moniker,
    solana_cli_config::{CONFIG_FILE, Config as SolanaCliConfig},
    solana_commitment_config::CommitmentConfig,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    spl_nonce_interface::state::Nonce,
    std::{io::ErrorKind, path::Path, str::FromStr},
};

#[derive(Debug)]
pub(crate) struct NonceAccount {
    pub(crate) state: Nonce,
    pub(crate) lamports: u64,
}

pub(crate) struct Client {
    rpc: RpcClient,
}

impl Client {
    pub(crate) fn new(args: ClientArgs) -> Result<Self> {
        let config = load_cli_config(args.config.as_deref())?;
        let url = match args.url {
            Some(url) => url,
            None => parse_url_or_moniker(&config.json_rpc_url)
                .map_err(|error| anyhow!(error))
                .context("invalid RPC URL in Solana CLI configuration")?,
        };
        let commitment = match args.commitment {
            Some(commitment) => commitment,
            None if config.commitment.is_empty() => CommitmentConfig::confirmed(),
            None => CommitmentConfig::from_str(&config.commitment).with_context(|| {
                format!(
                    "invalid commitment {:?} in Solana CLI configuration",
                    config.commitment
                )
            })?,
        };

        Ok(Self {
            rpc: RpcClient::new_with_commitment(url, commitment),
        })
    }

    pub(crate) async fn nonce_account(&self, address: &Address) -> Result<Option<NonceAccount>> {
        self.rpc
            .get_account_with_commitment(address, self.rpc.commitment())
            .await
            .with_context(|| format!("failed to fetch account {address}"))?
            .value
            .map(|account| decode_nonce_account(address, account))
            .transpose()
    }

}

fn decode_nonce_account(address: &Address, account: Account) -> Result<NonceAccount> {
    if account.owner != spl_nonce_interface::id() {
        bail!(
            "account {address} is owned by {}, not the SPL Nonce program {}",
            account.owner,
            spl_nonce_interface::id()
        );
    }
    let state = spl_nonce_client::state::decode(&account.data)
        .with_context(|| format!("account {address} contains invalid SPL Nonce data"))?;
    Ok(NonceAccount {
        state,
        lamports: account.lamports,
    })
}

fn load_cli_config(config_path: Option<&Path>) -> Result<SolanaCliConfig> {
    if let Some(config_path) = config_path {
        let config_path = config_path
            .to_str()
            .ok_or_else(|| anyhow!("Solana CLI config path is not valid UTF-8"))?;
        return SolanaCliConfig::load(config_path)
            .with_context(|| format!("failed to load Solana CLI config {config_path}"));
    }

    let Some(config_path) = CONFIG_FILE.as_deref() else {
        return Ok(SolanaCliConfig::default());
    };

    match SolanaCliConfig::load(config_path) {
        Ok(config) => Ok(config),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(SolanaCliConfig::default()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to load Solana CLI config {config_path}"))
        }
    }
}
