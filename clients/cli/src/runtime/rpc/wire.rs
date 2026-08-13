use {
    anyhow::{Context, Result, bail},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde::Deserialize,
    serde_json::Value,
    solana_address::Address,
};

#[derive(Debug, Deserialize)]
pub(crate) struct RpcEnvelope<T> {
    pub(crate) result: Option<T>,
    pub(crate) error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LatestBlockhashResponse {
    pub(crate) value: LatestBlockhashValue,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LatestBlockhashValue {
    pub(crate) blockhash: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountInfoResponse {
    pub(crate) value: Option<RpcAccountWire>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RpcAccountWire {
    lamports: u64,
    owner: String,
    data: Vec<String>,
}

impl RpcAccountWire {
    pub(crate) fn try_into_account(self) -> Result<RpcAccount> {
        let Some(encoded_data) = self.data.first() else {
            bail!("RPC account data did not include base64 bytes");
        };
        let data = STANDARD
            .decode(encoded_data)
            .context("RPC account data was not valid base64")?;
        let owner = self
            .owner
            .parse::<Address>()
            .with_context(|| format!("RPC account owner was invalid: {}", self.owner))?;
        Ok(RpcAccount {
            lamports: self.lamports,
            owner,
            data,
        })
    }
}

pub(crate) struct RpcAccount {
    pub(crate) lamports: u64,
    pub(crate) owner: Address,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignatureStatusesResponse {
    pub(crate) value: Vec<Option<SignatureStatus>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignatureStatus {
    pub(crate) err: Option<Value>,
    pub(crate) confirmation_status: Option<String>,
}
