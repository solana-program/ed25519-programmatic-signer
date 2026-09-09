//! Solana CLI sign-only output import helpers.

use {
    crate::{Error, Result},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde::{Deserialize, Serialize},
    solana_hash::Hash,
    solana_message::VersionedMessage,
};

/// Output from Solana-style `--sign-only --dump-transaction-message --output json`.
///
/// The `message` field is the compatibility contract: Solana CLI and SPL Token CLI
/// emit a base64 encoded transaction message when `--dump-transaction-message` is set.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOnlyTransaction {
    /// Blockhash printed by the source CLI. In the durable-nonce pattern it carries
    /// the nonce value that becomes the inner message's lifetime specifier.
    pub blockhash: String,
    /// Base64 encoded binary transaction message. Absent unless the source command used
    /// `--dump-transaction-message`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Valid signer pairs emitted by the source CLI. These are diagnostic only here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signers: Vec<String>,
    /// Required signer pubkeys that the source CLI could not sign.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
    /// Signer pubkeys whose source CLI signatures did not verify.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bad_sig: Vec<String>,
}

impl SignOnlyTransaction {
    /// Parses sign-only JSON emitted by Solana CLI or SPL Token CLI.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|_| Error::InvalidJson)
    }

    /// Decodes and validates the dumped transaction message.
    pub fn message(&self) -> Result<VersionedMessage> {
        if !self.bad_sig.is_empty() {
            return Err(Error::BadSignOnlySignatures);
        }
        let message_base64 = self
            .message
            .as_ref()
            .ok_or(Error::MissingTransactionMessage)?;
        let message_bytes = STANDARD
            .decode(message_base64)
            .map_err(|_| Error::InvalidBase64)?;
        let message = wincode::deserialize_exact::<VersionedMessage>(&message_bytes)
            .map_err(|_| Error::InvalidInnerMessage)?;
        let blockhash = self
            .blockhash
            .parse::<Hash>()
            .map_err(|_| Error::InvalidJson)?;
        if message.recent_blockhash() != &blockhash {
            return Err(Error::SignOnlyLifetimeMismatch);
        }

        Ok(message)
    }
}
