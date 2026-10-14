use {
    super::{RequiredSigners, execution_message, new_owner_signer},
    crate::{client::Client, output::OutputFormat},
    anyhow::{Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_loader_v3_interface::{
        instruction::set_upgrade_authority_checked, state::UpgradeableLoaderState,
    },
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct ProgramCommand {
    /// Program to migrate. Only programs deployed with the upgradeable loader (v3) are supported.
    program: Address,

    /// Authority of the new owner. Its programmatic signer becomes the upgrade authority. Pass
    /// the authority itself, not its programmatic signer.
    #[clap(long)]
    new_owner: Address,
}

pub(super) async fn run(
    command: ProgramCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let new_owner_signer = new_owner_signer(&command.new_owner)?;
    let UpgradeableLoaderState::Program {
        programdata_address,
    } = client
        .upgradeable_loader_account(&command.program, "program")
        .await?
    else {
        bail!(
            "account {} is not a program. Pass the program address, not its program data or a \
             buffer",
            command.program
        );
    };
    let UpgradeableLoaderState::ProgramData {
        upgrade_authority_address,
        ..
    } = client
        .upgradeable_loader_account(&programdata_address, "program data account")
        .await?
    else {
        bail!("account {programdata_address} is not program data");
    };
    let Some(upgrade_authority) = upgrade_authority_address else {
        bail!(
            "program {} is immutable, it has no upgrade authority",
            command.program
        );
    };
    ensure!(
        upgrade_authority != new_owner_signer,
        "the upgrade authority of program {} is already {new_owner_signer}",
        command.program
    );

    let instruction =
        set_upgrade_authority_checked(&command.program, &upgrade_authority, &new_owner_signer);
    let execution_message = execution_message(&new_owner_signer, &[instruction])?;

    output.render(&ProgramMigrationOutput {
        program: command.program.to_string(),
        program_data: programdata_address.to_string(),
        new_owner: new_owner_signer.to_string(),
        new_owner_authority: command.new_owner.to_string(),
        required_signers: RequiredSigners::new(&execution_message, &[command.new_owner]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMigrationOutput {
    pub program: String,
    pub program_data: String,
    pub new_owner: String,
    pub new_owner_authority: String,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for ProgramMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Program: {}", self.program)?;
        writeln!(f, "Program data: {}", self.program_data)?;
        writeln!(f, "Authority moved: upgrade authority")?;
        writeln!(
            f,
            "New owner: {} (authority: {})",
            self.new_owner, self.new_owner_authority
        )?;
        writeln!(f)?;
        writeln!(f, "{}", self.required_signers)?;
        writeln!(f)?;
        write!(f, "Execution message (base64):\n{}", self.execution_message)
    }
}
