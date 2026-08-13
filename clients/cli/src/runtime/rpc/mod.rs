mod wire;

pub(crate) use wire::RpcAccount;
use {
    crate::runtime::{
        rpc::wire::{
            AccountInfoResponse, LatestBlockhashResponse, RpcEnvelope, SignatureStatusesResponse,
        },
        transaction::serialize_transaction_base64,
    },
    anyhow::{Context, Result, anyhow, bail},
    serde::Deserialize,
    serde_json::{Value, json},
    solana_address::Address,
    solana_hash::Hash,
    solana_transaction::versioned::VersionedTransaction,
    std::{thread, time::Duration},
};

const DEFAULT_CONFIRMATION_ATTEMPTS: usize = 30;
const CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const DEFAULT_ACCOUNT_READ_ATTEMPTS: usize = 20;
const ACCOUNT_READ_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct Rpc {
    url: String,
}

impl Rpc {
    pub(crate) fn new(url: String) -> Self {
        Self { url }
    }

    pub(crate) fn get_latest_blockhash(&self) -> Result<Hash> {
        let response: LatestBlockhashResponse = self.call("getLatestBlockhash", json!([]))?;
        response.value.blockhash.parse::<Hash>().with_context(|| {
            format!(
                "RPC returned invalid blockhash {}",
                response.value.blockhash
            )
        })
    }

    pub(crate) fn get_genesis_hash(&self) -> Result<Hash> {
        let genesis_hash: String = self.call("getGenesisHash", json!([]))?;
        genesis_hash
            .parse::<Hash>()
            .with_context(|| format!("RPC returned invalid genesis hash {genesis_hash}"))
    }

    pub(crate) fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        self.call("getMinimumBalanceForRentExemption", json!([data_len]))
    }

    pub(crate) fn get_account(&self, address: &Address) -> Result<RpcAccount> {
        let response: AccountInfoResponse = self.call(
            "getAccountInfo",
            json!([
                address.to_string(),
                {
                    "commitment": "confirmed",
                    "encoding": "base64",
                }
            ]),
        )?;
        let Some(account) = response.value else {
            bail!("account {address} was not found");
        };
        account.try_into_account()
    }

    pub(crate) fn get_account_with_retries(&self, address: &Address) -> Result<RpcAccount> {
        self.get_account_until(address, |_| Ok(true), "available")
    }

    pub(crate) fn get_account_until<F>(
        &self,
        address: &Address,
        mut is_ready: F,
        description: &str,
    ) -> Result<RpcAccount>
    where
        F: FnMut(&RpcAccount) -> Result<bool>,
    {
        let mut last_error = None;
        for _attempt in 0..DEFAULT_ACCOUNT_READ_ATTEMPTS {
            match self.get_account(address) {
                Ok(account) if is_ready(&account)? => return Ok(account),
                Ok(_account) => {}
                Err(error) => last_error = Some(error),
            }
            thread::sleep(ACCOUNT_READ_POLL_INTERVAL);
        }

        if let Some(error) = last_error {
            return Err(error).with_context(|| format!("account {address} was not {description}"));
        }
        bail!("account {address} was not {description} before timeout")
    }

    pub(crate) fn send_transaction(
        &self,
        transaction: &VersionedTransaction,
        skip_preflight: bool,
    ) -> Result<String> {
        self.call(
            "sendTransaction",
            json!([
                serialize_transaction_base64(transaction)?,
                {
                    "encoding": "base64",
                    "skipPreflight": skip_preflight,
                    "preflightCommitment": "confirmed",
                }
            ]),
        )
    }

    pub(crate) fn simulate_transaction(
        &self,
        transaction: &VersionedTransaction,
        verify_signatures: bool,
    ) -> Result<Value> {
        self.call(
            "simulateTransaction",
            json!([
                serialize_transaction_base64(transaction)?,
                {
                    "encoding": "base64",
                    "sigVerify": verify_signatures,
                    "replaceRecentBlockhash": !verify_signatures,
                }
            ]),
        )
    }

    pub(crate) fn confirm_signature(&self, signature: &str) -> Result<()> {
        for _attempt in 0..DEFAULT_CONFIRMATION_ATTEMPTS {
            let response: SignatureStatusesResponse = self.call(
                "getSignatureStatuses",
                json!([[signature], {"searchTransactionHistory": false}]),
            )?;
            let Some(Some(status)) = response.value.into_iter().next() else {
                thread::sleep(CONFIRMATION_POLL_INTERVAL);
                continue;
            };
            if let Some(error) = status.err {
                bail!("transaction {signature} failed: {error}");
            }
            if status.confirmation_status.as_deref() == Some("confirmed")
                || status.confirmation_status.as_deref() == Some("finalized")
            {
                return Ok(());
            }
            thread::sleep(CONFIRMATION_POLL_INTERVAL);
        }
        bail!("transaction {signature} was not confirmed before timeout")
    }

    fn call<T>(&self, method: &str, params: Value) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let mut response = ureq::post(&self.url)
            .send_json(&request)
            .with_context(|| format!("RPC request {method} failed for {}", self.url))?;
        let envelope: RpcEnvelope<T> = response
            .body_mut()
            .read_json()
            .with_context(|| format!("RPC response {method} was not valid JSON"))?;
        if let Some(error) = envelope.error {
            bail!("RPC {method} failed: {} ({})", error.message, error.code);
        }
        envelope
            .result
            .ok_or_else(|| anyhow!("RPC {method} response did not include result"))
    }
}
