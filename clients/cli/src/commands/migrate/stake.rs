use {
    super::{RequiredSigners, execution_message, new_owner_signer},
    crate::{client::Client, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::{Args, ValueEnum},
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_stake_interface::{instruction::authorize_checked, state::StakeAuthorize},
    std::fmt,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AuthorityType {
    Staker,
    Withdrawer,
}

#[derive(Debug, Args)]
pub(crate) struct StakeCommand {
    /// Stake account to migrate.
    stake_account: Address,

    /// Authority of the new owner. Its programmatic signer becomes the stake authority. Pass the
    /// authority itself, not its programmatic signer.
    #[clap(long)]
    new_owner: Address,

    /// Move only this stake authority. Moves both the staker and the withdrawer when omitted.
    #[clap(long, value_enum)]
    authority_type: Option<AuthorityType>,
}

pub(super) async fn run(
    command: StakeCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let new_owner_signer = new_owner_signer(&command.new_owner)?;
    let meta = client
        .stake_account(&command.stake_account)
        .await?
        .meta()
        .with_context(|| format!("stake account {} is not initialized", command.stake_account))?;
    let roles: &[StakeAuthorize] = match command.authority_type {
        None => &[StakeAuthorize::Staker, StakeAuthorize::Withdrawer],
        Some(AuthorityType::Staker) => &[StakeAuthorize::Staker],
        Some(AuthorityType::Withdrawer) => &[StakeAuthorize::Withdrawer],
    };
    // Changing the withdrawer during a lockup also needs the custodian. Whether a lockup is in
    // force is checked against the current state.
    let custodian = if roles.contains(&StakeAuthorize::Withdrawer) {
        let clock = client.clock().await?;
        meta.lockup
            .is_in_force(&clock, None)
            .then_some(meta.lockup.custodian)
    } else {
        None
    };

    let instructions = roles
        .iter()
        .map(|role| {
            let (current, custodian) = match role {
                StakeAuthorize::Staker => (meta.authorized.staker, None),
                StakeAuthorize::Withdrawer => (meta.authorized.withdrawer, custodian.as_ref()),
            };
            ensure!(
                current != new_owner_signer,
                "the {} of stake account {} is already {new_owner_signer}",
                role_name(*role),
                command.stake_account
            );
            Ok(authorize_checked(
                &command.stake_account,
                &current,
                &new_owner_signer,
                *role,
                custodian,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let execution_message = execution_message(&new_owner_signer, &instructions)?;

    output.render(&StakeMigrationOutput {
        stake_account: command.stake_account.to_string(),
        authority_types: roles
            .iter()
            .map(|role| role_name(*role).to_string())
            .collect(),
        new_owner: new_owner_signer.to_string(),
        new_owner_authority: command.new_owner.to_string(),
        custodian: custodian.as_ref().map(ToString::to_string),
        required_signers: RequiredSigners::new(&execution_message, &[command.new_owner]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

pub(super) fn role_name(role: StakeAuthorize) -> &'static str {
    match role {
        StakeAuthorize::Staker => "staker",
        StakeAuthorize::Withdrawer => "withdrawer",
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakeMigrationOutput {
    pub stake_account: String,
    pub authority_types: Vec<String>,
    pub new_owner: String,
    pub new_owner_authority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custodian: Option<String>,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for StakeMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Stake account: {}", self.stake_account)?;
        writeln!(f, "Authorities moved: {}", self.authority_types.join(", "))?;
        writeln!(
            f,
            "New owner: {} (authority: {})",
            self.new_owner, self.new_owner_authority
        )?;
        if let Some(custodian) = &self.custodian {
            writeln!(f, "Lockup custodian: {custodian}")?;
        }
        writeln!(f)?;
        writeln!(f, "{}", self.required_signers)?;
        writeln!(f)?;
        write!(f, "Execution message (base64):\n{}", self.execution_message)
    }
}
