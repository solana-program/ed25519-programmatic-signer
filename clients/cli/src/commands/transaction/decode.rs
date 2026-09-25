use {
    anyhow::{Context, Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_message::{VersionedMessage, legacy::Message},
};

/// Decode and sanitize a base64-encoded message. `name` describes the message in errors.
pub(super) fn read_message(input: &str, name: &str) -> Result<VersionedMessage> {
    let bytes = BASE64_STANDARD
        .decode(input.trim())
        .with_context(|| format!("invalid base64 {name}"))?;
    let message: VersionedMessage =
        wincode::deserialize_exact(&bytes).with_context(|| format!("invalid serialized {name}"))?;
    message
        .sanitize()
        .with_context(|| format!("invalid {name}"))?;
    Ok(message)
}

/// Read an inner message the executor can invoke.
pub(super) fn read_inner_message(input: &str) -> Result<Message> {
    let VersionedMessage::Legacy(message) = read_message(input, "inner message")? else {
        bail!("the executor supports only legacy inner messages");
    };
    ensure!(
        !message.has_duplicates(),
        "inner message must not contain duplicate account keys"
    );
    Ok(message)
}
