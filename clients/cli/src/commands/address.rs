use {
    crate::{
        cli::OutputFormat, presentation::render::render, runtime::signer::programmatic_signer,
    },
    anyhow::Result,
    clap::Args,
    serde_json::json,
    solana_address::Address,
};

#[derive(Debug, Args)]
pub(crate) struct AddressCommand {
    #[arg(value_name = "AUTHORITY")]
    pub(crate) authority: Address,
}

pub(crate) fn run(command: AddressCommand, output: OutputFormat) -> Result<String> {
    let programmatic_signer = programmatic_signer(&command.authority);
    render(
        output,
        || programmatic_signer.to_string(),
        || {
            json!({
                "authority": command.authority.to_string(),
                "programmaticSigner": programmatic_signer.to_string(),
                "signerProgram": spl_ed25519_signer_interface::id().to_string(),
            })
        },
    )
}
