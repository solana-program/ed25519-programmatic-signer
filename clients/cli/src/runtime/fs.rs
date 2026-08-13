use {
    crate::runtime::transaction::{deserialize_transaction_base64, serialize_transaction_base64},
    anyhow::{Context, Result, anyhow},
    solana_transaction::versioned::VersionedTransaction,
    std::{
        fs,
        io::{self, Read as _},
        path::{Path, PathBuf},
    },
};

pub(crate) fn read_transaction(path: &Path) -> Result<VersionedTransaction> {
    deserialize_transaction_base64(&read_text(path)?).with_context(|| {
        if path == Path::new("-") {
            String::from("failed to decode transaction from stdin")
        } else {
            format!("failed to decode transaction {}", path.display())
        }
    })
}

pub(crate) fn read_text(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read stdin")?;
        return Ok(buffer);
    }
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

pub(crate) fn write_transaction(
    path: Option<&PathBuf>,
    transaction: &VersionedTransaction,
) -> Result<String> {
    let encoded = serialize_transaction_base64(transaction)?;
    write_payload(path, &encoded, "transaction")
}

pub(crate) fn write_submit_transaction(
    path: Option<&PathBuf>,
    transaction: &VersionedTransaction,
) -> Result<String> {
    let encoded = serialize_transaction_base64(transaction)?;
    write_payload(path, &encoded, "submit transaction")
}

pub(crate) fn write_payload(path: Option<&PathBuf>, payload: &str, label: &str) -> Result<String> {
    let Some(path) = path else {
        return Ok(payload.to_string());
    };
    if path == Path::new("-") {
        return Ok(payload.to_string());
    }
    fs::write(path, payload).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(format!("wrote {label} to {}", path.display()))
}

pub(crate) fn batch_output_path(
    outfile: Option<&PathBuf>,
    outdir: Option<&PathBuf>,
    input: &Path,
) -> Result<Option<PathBuf>> {
    if let Some(outfile) = outfile {
        return Ok(Some(outfile.clone()));
    }
    let Some(outdir) = outdir else {
        return Ok(None);
    };
    let file_name = input
        .file_name()
        .ok_or_else(|| anyhow!("batch signing requires file paths with names"))?;
    Ok(Some(outdir.join(file_name)))
}
