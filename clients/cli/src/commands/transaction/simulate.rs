use {
    crate::{
        context::CliContext,
        presentation::{render::render, simulation::format_simulation},
        runtime::{
            fs::read_transaction,
            signer::{read_signer, read_signers, signer_refs},
            transaction::unsigned_transaction,
        },
    },
    anyhow::Result,
    clap::{Args, Subcommand, ValueHint},
    spl_programmatic_signer_rust::{inspect, submit::submit_transaction},
    std::path::PathBuf,
};

#[derive(Debug, Args)]
pub(crate) struct TransactionSimulateCommand {
    #[command(subcommand)]
    pub(crate) mode: TransactionSimulateMode,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TransactionSimulateMode {
    /// Simulate the inner message only.
    #[command(alias = "inner-message")]
    Inner(TransactionSimulateInnerCommand),
    /// Build and simulate the hot relay transaction.
    #[command(alias = "hot-relay-transaction")]
    #[command(alias = "relay-transaction")]
    Relay(TransactionSimulateRelayCommand),
}

#[derive(Debug, Args)]
pub(crate) struct TransactionSimulateInnerCommand {
    #[arg(
        value_name = "TRANSACTION_FILE",
        value_hint = ValueHint::FilePath,
        help = "Transaction file whose inner message should be simulated"
    )]
    pub(crate) transaction: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct TransactionSimulateRelayCommand {
    #[arg(
        value_name = "TRANSACTION_FILE",
        value_hint = ValueHint::FilePath,
        help = "Signed transaction file to wrap in a hot relay transaction"
    )]
    pub(crate) transaction: PathBuf,
    #[arg(
        long,
        value_name = "KEYPAIR_OR_URL",
        help = "Fee payer signer source for the simulated hot relay transaction"
    )]
    pub(crate) fee_payer: String,
    #[arg(
        long = "submit-signer",
        value_name = "KEYPAIR_OR_URL",
        help = "Required live submit signer source for designated-relayer files"
    )]
    pub(crate) submit_signers: Vec<String>,
}

pub(super) fn run(command: TransactionSimulateCommand, context: &CliContext) -> Result<String> {
    let rpc = context.rpc()?;
    let (transaction, verify_signatures) = match command.mode {
        TransactionSimulateMode::Inner(command) => {
            let wrapped_transaction = read_transaction(&command.transaction)?;
            let summary = inspect(&wrapped_transaction)?;
            (unsigned_transaction(summary.inner_message), false)
        }
        TransactionSimulateMode::Relay(command) => {
            let wrapped_transaction = read_transaction(&command.transaction)?;
            let mut wallet_manager = None;
            let fee_payer = read_signer(&command.fee_payer, "fee-payer", &mut wallet_manager)?;
            let submit_signers = read_signers(
                &command.submit_signers,
                "submit-signer",
                &mut wallet_manager,
            )?;
            let submit_signer_refs = signer_refs(&submit_signers);
            (
                submit_transaction(
                    &wrapped_transaction,
                    fee_payer.as_ref(),
                    &submit_signer_refs,
                    rpc.get_latest_blockhash()?,
                )?,
                true,
            )
        }
    };
    let simulation = rpc.simulate_transaction(&transaction, verify_signatures)?;
    let simulation_json = simulation.clone();
    render(
        context.output,
        || format_simulation(&simulation),
        || simulation_json,
    )
}
