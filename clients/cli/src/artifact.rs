//! Transaction-file IO. Files contain the Solana SDK's legacy Transaction JSON.
use {
    crate::transaction::WrappedTransaction,
    anyhow::{Context, Result, bail},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde::Deserialize,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy::Message},
    std::{
        fs::File,
        io::{self, Read, Write},
        path::Path,
    },
};

pub(crate) fn read_text(path: &Path) -> Result<String> {
    let reader: Box<dyn Read> = if path == Path::new("-") {
        Box::new(io::stdin())
    } else {
        Box::new(File::open(path).with_context(|| format!("failed to open {}", path.display()))?)
    };
    let mut text = String::new();
    reader.take(1_048_577).read_to_string(&mut text)?;
    if text.len() > 1_048_576 {
        bail!("input exceeds the 1 MiB transaction-file limit");
    }
    Ok(text)
}

pub(crate) fn read(path: &Path) -> Result<WrappedTransaction> {
    WrappedTransaction::from_json(&read_text(path)?)
}

pub(crate) fn write(path: Option<&Path>, transaction: &WrappedTransaction) -> Result<String> {
    let payload = transaction.to_json()?;
    let Some(path) = path.filter(|path| *path != Path::new("-")) else {
        return Ok(payload);
    };
    // Input and signed copies remain separate; a typo must not overwrite an existing file.
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create {}; choose a new output path", path.display()))?;
    writeln!(file, "{payload}").with_context(|| format!("failed to write {}", path.display()))?;
    Ok(format!("Wrote {}", path.display()))
}

pub(crate) fn read_message(path: &Path) -> Result<Message> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SignOnly {
        blockhash: String,
        message: Option<String>,
        #[serde(default)]
        bad_sig: Vec<String>,
    }
    let source: SignOnly =
        serde_json::from_str(&read_text(path)?).context("invalid sign-only JSON")?;
    anyhow::ensure!(
        source.bad_sig.is_empty(),
        "sign-only output contains bad signatures"
    );
    let bytes = STANDARD
        .decode(
            source
                .message
                .context("missing dumped transaction message")?,
        )
        .context("invalid base64 message")?;
    let message: VersionedMessage =
        wincode::deserialize_exact(&bytes).context("invalid message bytes")?;
    let VersionedMessage::Legacy(message) = message else {
        bail!("only legacy inner messages are supported");
    };
    anyhow::ensure!(
        message.recent_blockhash == source.blockhash.parse::<Hash>()?,
        "sign-only blockhash does not match the message lifetime specifier"
    );
    Ok(message)
}
