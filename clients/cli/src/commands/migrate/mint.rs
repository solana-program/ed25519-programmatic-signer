use {
    super::{
        RequiredSigners, default_new_authority, execution_message, new_owner_programmatic_signer,
        warn_kept_authorities,
    },
    crate::{
        client::Client,
        mint_authority::{AuthorityType, authorities},
        output::OutputFormat,
    },
    anyhow::{Context, Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::{Args, ValueEnum},
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_instruction::Instruction,
    spl_memo_interface::instruction::build_memo,
    spl_token_2022_interface::instruction::set_authority,
    spl_token_group_interface::instruction::update_group_authority,
    spl_token_metadata_interface::instruction::update_authority,
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct MintCommand {
    /// Token or Token-2022 mint to migrate.
    mint: Address,

    /// Signing pubkey of the new authority. Its programmatic signer becomes the mint's authorities.
    /// Pass the signing pubkey itself, not its programmatic signer. Defaults to the current
    /// authority, which then moves to its own programmatic signer.
    #[clap(long)]
    new_authority_signing_pubkey: Option<Address>,

    /// Move only these authorities. Repeat, or separate with commas, to move several in one
    /// execution message. All but mint and freeze belong to Token-2022 extensions. Moves every
    /// authority the mint has when omitted, skipping any that are not set.
    #[clap(
        long = "authority-type",
        value_name = "AUTHORITY_TYPE",
        value_enum,
        value_delimiter = ','
    )]
    authority_types: Vec<AuthorityType>,
}

pub(super) async fn run(
    command: MintCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let account = client.token_program_account(&command.mint, "mint").await?;
    let token_program = account.owner;
    let authorities = authorities(&account.data)
        .with_context(|| format!("account {} is not a mint", command.mint))?;

    // An unset authority can never be set again, so there is nothing to move.
    let (roles, unset): (Vec<_>, Vec<_>) = if command.authority_types.is_empty() {
        authorities
            .keys()
            .copied()
            .partition(|role| authorities[role].is_some())
    } else {
        let roles = AuthorityType::value_variants()
            .iter()
            .copied()
            .filter(|role| command.authority_types.contains(role))
            .collect();
        (roles, vec![])
    };
    ensure!(
        !roles.is_empty(),
        "mint {} has no authorities set",
        command.mint
    );
    let current_authorities = roles
        .iter()
        .map(|role| match authorities.get(role) {
            Some(authority) => authority
                .with_context(|| format!("mint {} has no {} authority", command.mint, role.name())),
            None => {
                // Only the mint and freeze authorities are always present.
                let extension = role.extension().unwrap();
                if token_program == spl_token_2022_interface::id() {
                    bail!(
                        "mint {} has no {extension:?} extension, so it has no {} authority",
                        command.mint,
                        role.name()
                    )
                } else {
                    bail!(
                        "mint {} is not a Token-2022 mint, so it has no {} authority",
                        command.mint,
                        role.name()
                    )
                }
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let new_authority_signing_pubkey = match command.new_authority_signing_pubkey {
        Some(new_authority_signing_pubkey) => new_authority_signing_pubkey,
        None => {
            // The authorities being moved must share one to default to it.
            let current = current_authorities[0];
            if let Some((role, other)) = roles
                .iter()
                .zip(&current_authorities)
                .find(|(_, authority)| **authority != current)
            {
                bail!(
                    "the {} authority {current} and {} authority {other} of mint {} differ, so \
                     neither can be the default new authority signing pubkey. Pass \
                     --new-authority-signing-pubkey, or move one with --authority-type",
                    roles[0].name(),
                    role.name(),
                    command.mint
                );
            }
            default_new_authority(
                format_args!("the current authority of mint {}", command.mint),
                current,
            )?
        }
    };
    let new_owner_programmatic_signer =
        new_owner_programmatic_signer(&new_authority_signing_pubkey)?;

    let mut instructions = roles
        .iter()
        .zip(&current_authorities)
        .map(|(role, current)| {
            ensure!(
                *current != new_owner_programmatic_signer,
                "the {} authority of mint {} is already {new_owner_programmatic_signer}",
                role.name(),
                command.mint
            );
            move_authority(
                &token_program,
                &command.mint,
                *role,
                current,
                &new_owner_programmatic_signer,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    // We add a memo instruction with the new programmatic signer as a signer
    // None of these instructions require the new authority as a signer
    // We want to ensure that the user of the CLI can sign for the new authority
    instructions.push(build_memo(
        &spl_memo_interface::v4::id(),
        memo(&roles, &new_owner_programmatic_signer).as_bytes(),
        &[&new_owner_programmatic_signer],
    ));
    let execution_message = execution_message(&new_owner_programmatic_signer, &instructions)?;

    // Only possible with --authority-type, since otherwise every set authority moves.
    warn_kept_authorities(
        format_args!("mint {}", command.mint),
        authorities.iter().filter_map(|(role, authority)| {
            authority
                .filter(|_| !roles.contains(role))
                .map(|authority| (role.name(), authority))
        }),
        &current_authorities,
    );

    output.render(&MintMigrationOutput {
        mint: command.mint.to_string(),
        token_program: token_program.to_string(),
        authority_types: roles.iter().map(|role| role.name().to_string()).collect(),
        unset_authority_types: unset.iter().map(|role| role.name().to_string()).collect(),
        new_authority: new_owner_programmatic_signer.to_string(),
        new_authority_signing_pubkey: new_authority_signing_pubkey.to_string(),
        required_signers: RequiredSigners::new(&execution_message, &[new_authority_signing_pubkey]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

/// Instruction moving the mint's `role` authority from `current` to `new`. Token-2022 processes
/// the metadata and group instructions too, since they are stored in the mint.
fn move_authority(
    token_program: &Address,
    mint: &Address,
    role: AuthorityType,
    current: &Address,
    new: &Address,
) -> Result<Instruction> {
    Ok(match role {
        AuthorityType::MetadataUpdate => {
            update_authority(token_program, mint, current, (*new).into())
        }
        AuthorityType::GroupUpdate => {
            update_group_authority(token_program, mint, current, Some(*new))
        }
        _ => set_authority(
            token_program,
            mint,
            Some(new),
            role.token_authority_type().unwrap(),
            current,
            &[],
        )
        .context("failed to build set authority instruction")?,
    })
}

/// Text of the memo the new authority's programmatic signer signs, such as "migrate mint, freeze
/// authority to <address>".
fn memo(roles: &[AuthorityType], new_owner_programmatic_signer: &Address) -> String {
    let names = roles.iter().map(|role| role.name()).collect::<Vec<_>>();
    format!(
        "migrate {} authority to {new_owner_programmatic_signer}",
        names.join(", ")
    )
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MintMigrationOutput {
    pub mint: String,
    pub token_program: String,
    pub authority_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unset_authority_types: Vec<String>,
    pub new_authority: String,
    pub new_authority_signing_pubkey: String,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for MintMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Mint: {}", self.mint)?;
        writeln!(f, "Token program: {}", self.token_program)?;
        writeln!(f, "Authorities moved: {}", self.authority_types.join(", "))?;
        if !self.unset_authority_types.is_empty() {
            writeln!(
                f,
                "Not set, skipped: {}",
                self.unset_authority_types.join(", ")
            )?;
        }
        writeln!(
            f,
            "New authority: {} (signing pubkey: {})",
            self.new_authority, self.new_authority_signing_pubkey
        )?;
        writeln!(f)?;
        writeln!(f, "{}", self.required_signers)?;
        writeln!(f)?;
        write!(f, "Execution message (base64):\n{}", self.execution_message)
    }
}
