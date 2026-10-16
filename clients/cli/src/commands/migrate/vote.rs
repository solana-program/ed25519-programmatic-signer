use {
    super::{RequiredSigners, execution_message, new_owner_signer},
    crate::{client::Client, output::OutputFormat},
    anyhow::{Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_vote_interface::{
        instruction::authorize_checked,
        state::{VoteAuthorize, VoteStateVersions},
    },
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct VoteCommand {
    /// Vote account to migrate.
    vote_account: Address,

    /// Authority of the new owner. Its programmatic signer becomes the withdraw authority. Pass
    /// the authority itself, not its programmatic signer.
    #[clap(long)]
    new_owner: Address,
}

pub(super) async fn run(
    command: VoteCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let new_owner_signer = new_owner_signer(&command.new_owner)?;
    // Only the withdrawer moves. The voter signs every vote and the node identity is the
    // validator's own key, so neither can be a programmatic signer.
    let withdrawer = authorized_withdrawer(
        &client.vote_account(&command.vote_account).await?,
        &command.vote_account,
    )?;
    ensure!(
        withdrawer != new_owner_signer,
        "the withdrawer of vote account {} is already {new_owner_signer}",
        command.vote_account
    );

    let instruction = authorize_checked(
        &command.vote_account,
        &withdrawer,
        &new_owner_signer,
        VoteAuthorize::Withdrawer,
    );
    let execution_message = execution_message(&new_owner_signer, &[instruction])?;

    output.render(&VoteMigrationOutput {
        vote_account: command.vote_account.to_string(),
        new_owner: new_owner_signer.to_string(),
        new_owner_authority: command.new_owner.to_string(),
        required_signers: RequiredSigners::new(&execution_message, &[command.new_owner]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

pub(super) fn authorized_withdrawer(
    state: &VoteStateVersions,
    vote_account: &Address,
) -> Result<Address> {
    if state.is_uninitialized() {
        bail!("vote account {vote_account} is not initialized");
    }
    Ok(match state {
        VoteStateVersions::Uninitialized => unreachable!("checked above"),
        VoteStateVersions::V1_14_11(state) => state.authorized_withdrawer,
        VoteStateVersions::V3(state) => state.authorized_withdrawer,
        VoteStateVersions::V4(state) => state.authorized_withdrawer,
    })
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteMigrationOutput {
    pub vote_account: String,
    pub new_owner: String,
    pub new_owner_authority: String,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for VoteMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Vote account: {}", self.vote_account)?;
        writeln!(f, "Authority moved: withdrawer")?;
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
