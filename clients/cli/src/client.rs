use {
    crate::cli::ClientArgs,
    anyhow::{Context, Result, anyhow, bail},
    clap::ArgMatches,
    solana_account::Account,
    solana_address::Address,
    solana_clap_v3_utils::{input_parsers::parse_url_or_moniker, keypair::signer_from_path},
    solana_cli_config::{CONFIG_FILE, Config as SolanaCliConfig},
    solana_commitment_config::CommitmentConfig,
    solana_hash::Hash,
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_rpc_client_types::config::RpcSendTransactionConfig,
    solana_signer::Signer,
    solana_transaction::Transaction,
    spl_nonce_interface::state::Nonce,
    std::{cell::RefCell, io::ErrorKind, path::Path, rc::Rc, str::FromStr, time::Duration},
    tokio::time::{sleep, timeout},
};

const ACCOUNT_READ_TIMEOUT: Duration = Duration::from_secs(10);
const RPC_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(crate) struct NonceAccount {
    pub(crate) state: Nonce,
    pub(crate) lamports: u64,
}

pub(crate) struct Client {
    rpc: RpcClient,
    fee_payer: Option<String>,
    default_fee_payer: String,
    skip_preflight: bool,
    matches: ArgMatches,
    wallet_manager: RefCell<Option<Rc<RemoteWalletManager>>>,
}

impl Client {
    pub(crate) fn new(args: ClientArgs, matches: ArgMatches) -> Result<Self> {
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
            fee_payer: args.fee_payer,
            default_fee_payer: config.keypair_path,
            skip_preflight: args.skip_preflight,
            matches,
            wallet_manager: RefCell::new(None),
        })
    }

    pub(crate) fn fee_payer(&self) -> Result<Box<dyn Signer>> {
        let source = self.fee_payer.as_deref().unwrap_or(&self.default_fee_payer);
        self.signer(source, "fee payer")
    }

    pub(crate) fn signer(&self, source: &str, name: &str) -> Result<Box<dyn Signer>> {
        let mut wallet_manager = self.wallet_manager.borrow_mut();
        signer_from_path(&self.matches, source, name, &mut wallet_manager)
            .map_err(|error| anyhow!(error.to_string()))
            .with_context(|| format!("failed to load {name}"))
    }

    pub(crate) async fn minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        self.rpc
            .get_minimum_balance_for_rent_exemption(data_len)
            .await
            .context("failed to query rent exemption")
    }

    pub(crate) async fn latest_blockhash(&self) -> Result<Hash> {
        self.rpc
            .get_latest_blockhash()
            .await
            .context("failed to get latest blockhash")
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

    pub(crate) async fn wait_for_nonce_account(&self, address: &Address) -> Result<NonceAccount> {
        timeout(ACCOUNT_READ_TIMEOUT, async {
            loop {
                if let Some(account) = self.nonce_account(address).await? {
                    return Ok(account);
                }
                sleep(RPC_POLL_INTERVAL).await;
            }
        })
        .await
        .map_err(|_| anyhow!("nonce account {address} was not available before timeout"))?
    }

    pub(crate) async fn send_and_confirm_transaction(
        &self,
        transaction: &Transaction,
    ) -> Result<String> {
        self.rpc
            .send_and_confirm_transaction_with_config(
                transaction,
                self.rpc.commitment(),
                RpcSendTransactionConfig {
                    skip_preflight: self.skip_preflight,
                    preflight_commitment: Some(self.rpc.commitment().commitment),
                    ..RpcSendTransactionConfig::default()
                },
            )
            .await
            .map(|signature| signature.to_string())
            .context("failed to send and confirm transaction")
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
