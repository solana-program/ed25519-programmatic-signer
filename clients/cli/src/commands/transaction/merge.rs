use {
    crate::runtime::fs::{read_transaction, write_transaction},
    anyhow::{Result, bail},
    clap::{Args, ValueHint},
    spl_programmatic_signer_rust::merge_transactions,
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionMergeCommand {
    #[arg(value_name = "TRANSACTION", value_hint = ValueHint::FilePath, num_args = 2..)]
    pub(crate) transactions: Vec<PathBuf>,
    #[arg(long, value_name = "OUTFILE", value_hint = ValueHint::FilePath)]
    pub(crate) outfile: Option<PathBuf>,
}

pub(super) fn run(command: TransactionMergeCommand) -> Result<String> {
    let Some((first_path, rest)) = command.transactions.split_first() else {
        bail!("transaction merge requires at least two transactions");
    };
    if rest.is_empty() {
        bail!("transaction merge requires at least two transactions");
    }
    let mut merged = read_transaction(first_path)?;
    for path in rest {
        let transaction = read_transaction(path)?;
        merge_transactions(&mut merged, &transaction)?;
    }
    write_transaction(command.outfile.as_ref(), &merged)
}
