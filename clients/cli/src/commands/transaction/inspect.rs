use {
    crate::{
        cli::OutputFormat,
        presentation::{
            inspect::{format_transaction_summary, transaction_summary_json},
            render::render,
        },
        runtime::fs::read_transaction,
    },
    anyhow::Result,
    clap::{Args, ValueHint},
    spl_programmatic_signer_rust::inspect,
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionInspectCommand {
    #[arg(value_name = "TRANSACTION", value_hint = ValueHint::FilePath)]
    pub(crate) transaction: PathBuf,
}

pub(super) fn run(command: TransactionInspectCommand, output: OutputFormat) -> Result<String> {
    let transaction = read_transaction(&command.transaction)?;
    let summary = inspect(&transaction)?;
    render(
        output,
        || format_transaction_summary(&summary),
        || transaction_summary_json(&summary),
    )
}
