use {
    crate::output::OutputFormat, anyhow::Result, clap::Args, serde::Serialize,
    solana_address::Address, spl_ed25519_signer_client::ProgrammaticSigner, std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct AddressCommand {
    /// Cold authority address used to derive the programmatic signer.
    pub(crate) authority: Address,
}

pub(crate) fn run(command: AddressCommand, output: OutputFormat) -> Result<String> {
    let signer_program = spl_ed25519_signer_client::id();
    let programmatic_signer =
        ProgrammaticSigner::derive_address(&signer_program, &command.authority);
    output.render(&AddressOutput {
        authority: command.authority.to_string(),
        programmatic_signer: programmatic_signer.to_string(),
        signer_program: signer_program.to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AddressOutput {
    authority: String,
    programmatic_signer: String,
    signer_program: String,
}

impl fmt::Display for AddressOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.programmatic_signer.fmt(formatter)
    }
}
