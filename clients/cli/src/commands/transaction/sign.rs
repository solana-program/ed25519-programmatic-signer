use {
    crate::runtime::{
        fs::{batch_output_path, read_transaction, write_transaction},
        signer::read_signers,
    },
    anyhow::{Result, bail},
    clap::{Args, ValueHint},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionSignCommand {
    #[arg(
        value_name = "TRANSACTION",
        value_hint = ValueHint::FilePath,
        num_args = 1..,
        required = true
    )]
    pub(crate) transactions: Vec<PathBuf>,
    #[arg(
        long = "keypair",
        value_name = "KEYPAIR_OR_URL",
        required = true,
        help = "Cold authority signer source"
    )]
    pub(crate) keypairs: Vec<String>,
    #[arg(long, value_name = "OUTFILE", value_hint = ValueHint::FilePath)]
    pub(crate) outfile: Option<PathBuf>,
    #[arg(long, value_name = "OUTDIR", value_hint = ValueHint::DirPath)]
    pub(crate) outdir: Option<PathBuf>,
}

pub(super) fn run(command: TransactionSignCommand) -> Result<String> {
    if command.transactions.is_empty() {
        bail!("transaction sign requires at least one transaction");
    }
    if command.keypairs.is_empty() {
        bail!("transaction sign requires at least one --keypair");
    }
    if command.transactions.len() > 1 && command.outfile.is_some() {
        bail!("--outfile can only be used with one input transaction; use --outdir for batches");
    }
    if command.transactions.len() > 1 && command.outdir.is_none() {
        bail!("signing multiple transactions requires --outdir");
    }
    let mut wallet_manager = None;
    let signers = read_signers(&command.keypairs, "keypair", &mut wallet_manager)?;
    let mut written = Vec::with_capacity(command.transactions.len());
    for transaction_path in &command.transactions {
        let mut transaction = read_transaction(transaction_path)?;
        for signer in &signers {
            spl_programmatic_signer_rust::sign_transaction(&mut transaction, signer.as_ref())?;
        }
        let output_path = batch_output_path(
            command.outfile.as_ref(),
            command.outdir.as_ref(),
            transaction_path,
        )?;
        let result = write_transaction(output_path.as_ref(), &transaction)?;
        written.push(result);
    }
    Ok(written.join("\n"))
}
