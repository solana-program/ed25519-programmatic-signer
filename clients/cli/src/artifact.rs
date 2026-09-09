//! Transaction-file IO. Files contain standard base64-encoded Solana transaction bytes.
use {
    anyhow::{Context, Result, bail},
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_transaction::versioned::VersionedTransaction,
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

pub(crate) fn read(path: &Path) -> Result<VersionedTransaction> {
    let text = read_text(path)?;
    let bytes = STANDARD
        .decode(text.trim())
        .context("transaction file must contain base64")?;
    let transaction = wincode::deserialize_exact(&bytes)
        .context("invalid transaction bytes; rebuild files from older deployments")?;
    spl_programmatic_signer_client::verify_static(&transaction)?;
    Ok(transaction)
}

pub(crate) fn write(path: Option<&Path>, transaction: &VersionedTransaction) -> Result<String> {
    let payload =
        STANDARD.encode(wincode::serialize(transaction).context("failed to encode transaction")?);
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
