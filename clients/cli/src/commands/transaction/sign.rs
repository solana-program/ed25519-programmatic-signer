use {
    crate::{artifact, client::Client},
    anyhow::{Context, Result, bail},
    clap::Args,
    std::{collections::HashSet, path::PathBuf},
};

#[derive(Debug, Args)]
pub(crate) struct SignCommand {
    #[clap(required = true)]
    transactions: Vec<PathBuf>,
    #[clap(long = "keypair", required = true, multiple_occurrences = true)]
    keypairs: Vec<String>,
    #[clap(long, conflicts_with = "outdir")]
    outfile: Option<PathBuf>,
    /// Existing directory for signed copies of a batch. Files are never overwritten.
    #[clap(long)]
    outdir: Option<PathBuf>,
}

pub(super) fn run(command: SignCommand, client: &Client) -> Result<String> {
    if command.transactions.len() > 1 && command.outdir.is_none() {
        bail!("batch signing requires --outdir");
    }
    let mut paths = HashSet::new();
    let mut transactions = Vec::new();
    for path in &command.transactions {
        let output = match &command.outdir {
            Some(dir) => Some(
                dir.join(
                    path.file_name()
                        .context("batch signing requires named files")?,
                ),
            ),
            None => command.outfile.clone(),
        };
        if let Some(output) = &output {
            if output.exists() || !paths.insert(output.clone()) {
                bail!("output {} already exists or is repeated", output.display());
            }
        }
        transactions.push((artifact::read(path)?, output));
    }
    let signers = command
        .keypairs
        .iter()
        .map(|source| client.signer(source, "cold signer"))
        .collect::<Result<Vec<_>>>()?;
    for (transaction, _) in &mut transactions {
        for signer in &signers {
            transaction.sign(signer.as_ref())?;
        }
    }
    transactions
        .iter()
        .map(|(transaction, path)| artifact::write(path.as_deref(), transaction))
        .collect::<Result<Vec<_>>>()
        .map(|lines| lines.join("\n"))
}

#[derive(Debug, Args)]
pub(crate) struct MergeCommand {
    #[clap(required = true, min_values = 2)]
    transactions: Vec<PathBuf>,
    #[clap(long)]
    outfile: Option<PathBuf>,
}

pub(super) fn merge(command: MergeCommand) -> Result<String> {
    let mut transaction = artifact::read(&command.transactions[0])?;
    for path in &command.transactions[1..] {
        transaction.merge(&artifact::read(path)?)?;
    }
    artifact::write(command.outfile.as_deref(), &transaction)
}
