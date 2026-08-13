use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        presentation::nonce::{NonceCreateOutput, render_create},
        runtime::{
            config::keypair_source,
            signer::{programmatic_signer, read_keypair, read_signer},
            transaction::sign_transaction,
        },
    },
    anyhow::{Result, bail},
    clap::{ArgGroup, Args, ValueHint},
    solana_address::Address,
    solana_signer::Signer,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::nonce::create_nonce_account_instructions,
    std::path::PathBuf,
};

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("nonce_authority_source")
        .required(true)
        .args(["nonce_authority", "programmatic_authority"])
))]
pub(crate) struct NonceCreateCommand {
    #[arg(
        long,
        value_name = "ADDRESS",
        conflicts_with = "programmatic_authority"
    )]
    pub(crate) nonce_authority: Option<Address>,
    #[arg(
        long,
        value_name = "COLD_AUTHORITY",
        conflicts_with = "nonce_authority"
    )]
    pub(crate) programmatic_authority: Option<Address>,
    #[arg(long, value_name = "KEYPAIR_PATH", value_hint = ValueHint::FilePath)]
    pub(crate) nonce_keypair: PathBuf,
    #[arg(long, value_name = "KEYPAIR_OR_URL", help = "Fee payer signer source")]
    pub(crate) fee_payer: Option<String>,
    #[arg(long)]
    pub(crate) skip_preflight: bool,
}

pub(super) fn run(command: NonceCreateCommand, context: &CliContext) -> Result<String> {
    let rpc = context.rpc()?;
    let fee_payer_source = keypair_source(command.fee_payer.as_deref())?;
    let mut wallet_manager = None;
    let fee_payer = read_signer(&fee_payer_source, "fee-payer", &mut wallet_manager)?;
    let nonce_keypair = read_keypair(&command.nonce_keypair)?;
    let nonce_authority = match (command.nonce_authority, command.programmatic_authority) {
        (Some(nonce_authority), None) => nonce_authority,
        (None, Some(authority)) => programmatic_signer(&authority),
        (None, None) => {
            bail!("nonce create requires --nonce-authority or --programmatic-authority")
        }
        (Some(_), Some(_)) => {
            bail!("--nonce-authority and --programmatic-authority cannot be used together")
        }
    };
    let rent_lamports = rpc.get_minimum_balance_for_rent_exemption(Nonce::LEN)?;
    let recent_blockhash = rpc.get_latest_blockhash()?;
    let [create, initialize] = create_nonce_account_instructions(
        &fee_payer.pubkey(),
        &nonce_keypair.pubkey(),
        &nonce_authority,
        rent_lamports,
    );
    let nonce_signers: [&dyn Signer; 1] = [&nonce_keypair];
    let transaction = sign_transaction(
        &[create, initialize],
        fee_payer.as_ref(),
        &nonce_signers,
        recent_blockhash,
    )?;
    let signature = rpc.send_transaction(&transaction, command.skip_preflight)?;
    rpc.confirm_signature(&signature)?;
    let account = rpc.get_account_with_retries(&nonce_keypair.pubkey())?;
    let nonce = decode_nonce_account(&account)?;
    render_create(
        context.output,
        NonceCreateOutput {
            signature,
            nonce_account: nonce_keypair.pubkey().to_string(),
            authority: nonce.authority.to_string(),
            nonce: nonce.nonce.to_string(),
            lamports: account.lamports,
            rent_lamports,
        },
    )
}
