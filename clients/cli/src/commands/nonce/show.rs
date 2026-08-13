use {
    crate::{
        context::CliContext,
        nonce_account::decode_nonce_account,
        presentation::nonce::{NonceShowOutput, render_show},
    },
    anyhow::Result,
    clap::Args,
    solana_address::Address,
};

#[derive(Debug, Args)]
pub(crate) struct NonceShowCommand {
    #[arg(value_name = "NONCE_ACCOUNT")]
    pub(crate) nonce_account: Address,
}

pub(super) fn run(command: NonceShowCommand, context: &CliContext) -> Result<String> {
    let rpc = context.rpc()?;
    let account = rpc.get_account(&command.nonce_account)?;
    let nonce = decode_nonce_account(&account)?;
    render_show(
        context.output,
        NonceShowOutput {
            nonce_account: command.nonce_account.to_string(),
            authority: nonce.authority.to_string(),
            nonce: nonce.nonce.to_string(),
            lamports: account.lamports,
            owner: account.owner.to_string(),
        },
    )
}
