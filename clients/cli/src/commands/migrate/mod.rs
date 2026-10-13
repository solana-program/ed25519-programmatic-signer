pub(crate) mod stake;

use {
    crate::{client::Client, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    clap::{Args, Subcommand},
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::v1,
    spl_ed25519_signer_client::ProgrammaticSigner,
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct MigrateCommand {
    #[clap(subcommand)]
    command: MigrateSubcommand,
}

#[derive(Debug, Subcommand)]
enum MigrateSubcommand {
    /// Build an execution message moving a stake account's authorities to a programmatic signer.
    Stake(stake::StakeCommand),
}

pub(crate) async fn run(
    command: MigrateCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    match command.command {
        MigrateSubcommand::Stake(command) => stake::run(command, client, output).await,
    }
}

/// The programmatic signer that becomes the new owner. The new owner must be an Ed25519 key,
/// since only an Ed25519 key can sign authorization messages for its PDA.
fn new_owner_signer(new_owner: &Address) -> Result<Address> {
    ensure!(
        new_owner.is_on_curve(),
        "new owner {new_owner} is not an on-curve address. Pass the authority, not its \
         programmatic signer"
    );
    Ok(ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_client::id(),
        new_owner,
    ))
}

/// Compile the execution message for a migration to `new_owner_signer`.
///
/// An execution message pays no fee, but v1 requires a writable signer first. The new owner's
/// programmatic signer signs every migration, so using it adds no signer. Submit promotes it
/// rather than verifying a signature for it, so it does not need to be on curve.
fn execution_message(
    new_owner_signer: &Address,
    instructions: &[Instruction],
) -> Result<v1::Message> {
    // `transaction sign` replaces the blockhash with the SPL nonce value.
    let message = v1::Message::try_compile(new_owner_signer, instructions, Hash::default())
        .context("failed to compile execution message")?;
    message.validate().context("invalid execution message")?;
    Ok(message)
}

/// Who must sign an execution message, besides the nonce authority chosen in `transaction sign`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredSigners {
    /// Off-curve signers, which an authority promotes by signing with `transaction sign`.
    pub authorizations: Vec<Authorization>,
    /// On-curve signers, which sign the relay transaction with `transaction submit`.
    pub forwarded_signers: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub programmatic_signer: String,
    /// Known when the signer derives from an address the command was given. A PDA can't be
    /// traced back to its authority.
    pub authority: Option<String>,
}

impl RequiredSigners {
    fn new(execution_message: &v1::Message, known_authorities: &[Address]) -> Self {
        let signers = &execution_message.account_keys
            [..usize::from(execution_message.header.num_required_signatures)];
        let (forwarded_signers, programmatic_signers): (Vec<&Address>, Vec<_>) =
            signers.iter().partition(|address| address.is_on_curve());
        Self {
            authorizations: programmatic_signers
                .into_iter()
                .map(|programmatic_signer| Authorization {
                    programmatic_signer: programmatic_signer.to_string(),
                    authority: known_authorities
                        .iter()
                        .find(|authority| {
                            ProgrammaticSigner::derive_address(
                                &spl_ed25519_signer_client::id(),
                                authority,
                            ) == *programmatic_signer
                        })
                        .map(ToString::to_string),
                })
                .collect(),
            forwarded_signers: forwarded_signers.iter().map(ToString::to_string).collect(),
        }
    }
}

impl fmt::Display for RequiredSigners {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.authorizations.is_empty() {
            writeln!(
                f,
                "Programmatic signers (authorize with `transaction sign`):"
            )?;
            for authorization in &self.authorizations {
                match &authorization.authority {
                    Some(authority) => writeln!(
                        f,
                        "  {} (authority: {authority})",
                        authorization.programmatic_signer
                    )?,
                    None => writeln!(
                        f,
                        "  {} (authority unknown, must be a programmatic signer)",
                        authorization.programmatic_signer
                    )?,
                }
            }
        }
        if !self.forwarded_signers.is_empty() {
            writeln!(f, "Forwarded signers (sign at submission):")?;
            for address in &self.forwarded_signers {
                writeln!(f, "  {address}")?;
            }
        }
        write!(
            f,
            "The nonce authority also signs. It is chosen in `transaction sign`."
        )
    }
}
