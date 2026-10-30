use {
    super::{
        RequiredSigners, default_new_authority, execution_message, new_owner_programmatic_signer,
    },
    crate::{client::Client, output::OutputFormat},
    anyhow::{Context, Result, anyhow, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_account_decoder_client_types::token::real_number_string_trimmed,
    solana_address::Address,
    solana_instruction::Instruction,
    spl_associated_token_account_interface::{
        address::get_associated_token_address_with_program_id,
        instruction::create_associated_token_account_idempotent,
    },
    spl_memo_interface::instruction::build_memo,
    spl_token_2022_interface::{
        extension::{
            BaseStateWithExtensions, StateWithExtensions,
            confidential_transfer::ConfidentialTransferAccount,
            confidential_transfer_fee::ConfidentialTransferFeeAmount,
            cpi_guard::CpiGuard,
            default_account_state::DefaultAccountState,
            non_transferable::NonTransferable,
            transfer_fee::{
                TransferFeeAmount, TransferFeeConfig, instruction::harvest_withheld_tokens_to_mint,
            },
            transfer_hook,
        },
        instruction::{close_account, transfer_checked},
        state::{Account as TokenAccount, AccountState, Mint},
    },
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct TokenCommand {
    /// Token or Token-2022 mint of the tokens to migrate.
    mint: Address,

    /// Current owner of the tokens. They move from its associated token account. Defaults to the
    /// pubkey of the configured keypair.
    #[clap(long)]
    owner: Option<Address>,

    /// Signing pubkey of the new authority. The tokens move to its programmatic signer's associated
    /// token account. Pass the signing pubkey itself, not its programmatic signer. Defaults to the
    /// current owner, whose tokens then move to its own programmatic signer.
    #[clap(long)]
    new_authority_signing_pubkey: Option<Address>,

    /// Amount to move, in tokens using the mint's decimals, such as 1.5. Moves the whole balance
    /// when omitted, as read when the message is built.
    #[clap(long)]
    amount: Option<String>,

    /// Pays rent for the new authority's token account, if the migration creates it. Defaults to
    /// the current owner.
    #[clap(long)]
    funder: Option<Address>,

    /// Leave the owner's token account open. Without this, the migration closes it when its
    /// balance ends up at zero, including when it is already zero.
    #[clap(long)]
    no_close: bool,

    /// Receives the rent of the owner's token account if the migration closes it. Defaults to the
    /// new authority's programmatic signer.
    #[clap(long, conflicts_with = "no-close")]
    rent_recipient: Option<Address>,
}

pub(super) async fn run(
    command: TokenCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let owner = match command.owner {
        Some(owner) => owner,
        None => client.config_pubkey()?,
    };
    let new_authority_signing_pubkey = match command.new_authority_signing_pubkey {
        Some(new_authority_signing_pubkey) => new_authority_signing_pubkey,
        None => default_new_authority("the owner", owner)?,
    };
    let new_owner_programmatic_signer =
        new_owner_programmatic_signer(&new_authority_signing_pubkey)?;
    ensure!(
        owner != new_owner_programmatic_signer,
        "owner {owner} is already the new authority's programmatic signer"
    );

    let mint_account = client.token_program_account(&command.mint, "mint").await?;
    let token_program = mint_account.owner;
    let mint = StateWithExtensions::<Mint>::unpack(&mint_account.data)
        .with_context(|| format!("account {} is not a mint", command.mint))?;
    let decimals = mint.base.decimals;

    let source =
        get_associated_token_address_with_program_id(&owner, &command.mint, &token_program);
    let source_account = client
        .token_program_account(&source, "associated token account")
        .await?;
    let source_account = StateWithExtensions::<TokenAccount>::unpack(&source_account.data)
        .with_context(|| format!("account {source} is not a token account"))?;
    ensure!(
        !source_account.base.is_frozen(),
        "token account {source} is frozen"
    );
    // The executor calls the token program through CPI, which the CPI guard blocks.
    if let Ok(guard) = source_account.get_extension::<CpiGuard>() {
        ensure!(
            !bool::from(guard.lock_cpi),
            "token account {source} has the CPI guard enabled, which blocks transfers through the \
             message executor. Disable it first"
        );
    }

    let balance = source_account.base.amount;
    let amount = match &command.amount {
        Some(amount) => {
            let amount = parse_token_amount(amount, decimals)?;
            ensure!(amount > 0, "amount must be greater than zero");
            ensure!(
                amount <= balance,
                "token account {source} holds {}, less than the amount",
                real_number_string_trimmed(balance, decimals)
            );
            amount
        }
        None => {
            // Only check for balance > 0 if --no-close
            ensure!(
                balance > 0 || !command.no_close,
                "token account {source} has no balance, remove --no-close to close it"
            );
            balance
        }
    };
    let close = amount == balance && !command.no_close;
    let rent_recipient = command
        .rent_recipient
        .unwrap_or(new_owner_programmatic_signer);

    let destination = get_associated_token_address_with_program_id(
        &new_owner_programmatic_signer,
        &command.mint,
        &token_program,
    );
    let mut instructions = vec![];
    let mut funder = None;
    // Mint transfer rules don't apply if we're just closing a zero balance.
    if amount > 0 {
        ensure!(
            mint.get_extension::<NonTransferable>().is_err(),
            "mint {} is non-transferable",
            command.mint
        );
        // A transfer through a hook needs extra accounts that the hook program defines.
        if let Some(hook_program) = transfer_hook::get_program_id(&mint) {
            bail!(
                "mint {} has a transfer hook ({hook_program}), which is not supported",
                command.mint
            );
        }

        // Check if the destination is already a token account
        let destination_exists = match client.optional_account(&destination).await? {
            Some(account) if account.owner == token_program => {
                let account = StateWithExtensions::<TokenAccount>::unpack(&account.data)
                    .with_context(|| format!("account {destination} is not a token account"))?;
                ensure!(
                    !account.base.is_frozen(),
                    "token account {destination} is frozen"
                );
                true
            }
            _ => false,
        };
        if !destination_exists {
            if let Ok(default_state) = mint.get_extension::<DefaultAccountState>() {
                ensure!(
                    default_state.state != u8::from(AccountState::Frozen),
                    "mint {} creates token accounts frozen, so the new authority's token account \
                     {destination} would be frozen",
                    command.mint
                );
            }
            let payer = command.funder.unwrap_or(owner);
            // Idempotent, in case the account is created before the migration runs.
            instructions.push(create_associated_token_account_idempotent(
                &payer,
                &new_owner_programmatic_signer,
                &command.mint,
                &token_program,
            ));
            funder = Some(payer);
        }
        instructions.push(
            transfer_checked(
                &token_program,
                &source,
                &command.mint,
                &destination,
                &owner,
                &[],
                amount,
                decimals,
            )
            .context("failed to build transfer instruction")?,
        );
    }
    if close {
        instructions.extend(close_instructions(
            &source_account,
            &source,
            &command.mint,
            &token_program,
            &owner,
            &rent_recipient,
        )?);
    }
    // A transfer doesn't need the new authority's approval, but requiring it confirms the new
    // authority can sign before any tokens move.
    // Intentionally not including detail in this memo, transfer is parsed by explorers
    instructions.push(build_memo(
        &spl_memo_interface::v4::id(),
        b"migrate tokens",
        &[&new_owner_programmatic_signer],
    ));
    let execution_message = execution_message(&new_owner_programmatic_signer, &instructions)?;

    output.render(&TokenMigrationOutput {
        mint: command.mint.to_string(),
        token_program: token_program.to_string(),
        amount: real_number_string_trimmed(amount, decimals),
        source: source.to_string(),
        destination: destination.to_string(),
        funder: funder.map(|funder| funder.to_string()),
        transfer_fee: amount > 0 && mint.get_extension::<TransferFeeConfig>().is_ok(),
        rent_recipient: close.then(|| rent_recipient.to_string()),
        new_authority: new_owner_programmatic_signer.to_string(),
        new_authority_signing_pubkey: new_authority_signing_pubkey.to_string(),
        required_signers: RequiredSigners::new(&execution_message, &[new_authority_signing_pubkey]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

/// Instructions that close the owner's token account once its balance is zero, sending its rent to
/// `rent_recipient`.
/// Rejects an account that can't be closed.
fn close_instructions(
    source_account: &StateWithExtensions<TokenAccount>,
    source: &Address,
    mint: &Address,
    token_program: &Address,
    owner: &Address,
    rent_recipient: &Address,
) -> Result<Vec<Instruction>> {
    let cannot_close = |reason: &str| {
        anyhow!(
            "token account {source} {reason}, so it can't be closed. Pass --no-close to leave it \
             open"
        )
    };
    ensure!(
        rent_recipient != source,
        "token account {source} can't receive its own rent"
    );
    if let Ok(confidential) = source_account.get_extension::<ConfidentialTransferAccount>() {
        confidential
            .closable()
            .map_err(|_| cannot_close("has a confidential balance"))?;
    }
    if let Ok(confidential_fee) = source_account.get_extension::<ConfidentialTransferFeeAmount>() {
        confidential_fee
            .closable()
            .map_err(|_| cannot_close("has withheld confidential transfer fees"))?;
    }

    let mut instructions = vec![];
    // Withheld transfer fees block closing, but anyone can harvest them to the mint.
    if source_account
        .get_extension::<TransferFeeAmount>()
        .is_ok_and(|fees| u64::from(fees.withheld_amount) > 0)
    {
        instructions.push(
            harvest_withheld_tokens_to_mint(token_program, mint, &[source])
                .context("failed to build harvest instruction")?,
        );
    }
    instructions.push(
        close_account(
            token_program,
            source,
            rent_recipient,
            // Once set, only the close authority can close the account.
            &Option::from(source_account.base.close_authority).unwrap_or(*owner),
            &[],
        )
        .context("failed to build close instruction")?,
    );
    Ok(instructions)
}

/// Convert an amount in tokens, such as `1.5`, to base units using the mint's decimals.
fn parse_token_amount(amount: &str, decimals: u8) -> Result<u64> {
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    ensure!(
        !(whole.is_empty() && fraction.is_empty())
            && whole
                .chars()
                .chain(fraction.chars())
                .all(|c| c.is_ascii_digit()),
        "invalid amount {amount}"
    );
    let decimals = usize::from(decimals);
    ensure!(
        fraction.len() <= decimals,
        "amount {amount} has more than {decimals} decimal places"
    );
    format!("{whole}{fraction:0<decimals$}")
        .parse()
        .with_context(|| format!("amount {amount} is too large"))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenMigrationOutput {
    pub mint: String,
    pub token_program: String,
    /// In tokens, using the mint's decimals. Zero when the migration only closes a token account
    /// whose balance is already zero.
    pub amount: String,
    pub source: String,
    pub destination: String,
    /// Pays rent for the destination token account. Set only when the migration creates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funder: Option<String>,
    /// Whether the mint charges a transfer fee, which comes out of the amount.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transfer_fee: bool,
    /// Receives the source token account's rent. Set only when the migration closes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rent_recipient: Option<String>,
    pub new_authority: String,
    pub new_authority_signing_pubkey: String,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for TokenMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Mint: {}", self.mint)?;
        writeln!(f, "Token program: {}", self.token_program)?;
        writeln!(f, "Amount: {}", self.amount)?;
        if self.transfer_fee {
            writeln!(
                f,
                "The mint charges a transfer fee, so the new authority receives less than this \
                 amount."
            )?;
        }
        writeln!(f, "From: {}", self.source)?;
        match &self.funder {
            Some(funder) => writeln!(
                f,
                "To: {} (created, rent paid by {funder})",
                self.destination
            )?,
            None => writeln!(f, "To: {}", self.destination)?,
        }
        if let Some(rent_recipient) = &self.rent_recipient {
            writeln!(
                f,
                "Closes {}, sending its rent to {rent_recipient}",
                self.source
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

#[cfg(test)]
mod tests {
    use {super::parse_token_amount, test_case::test_case};

    #[test_case("1", 0 => 1)]
    #[test_case("1.5", 6 => 1_500_000)]
    #[test_case(".5", 2 => 50)]
    #[test_case("2.", 2 => 200)]
    #[test_case("0.000001", 6 => 1)]
    #[test_case("18446744073709551615", 0 => u64::MAX)]
    fn parses_token_amounts(amount: &str, decimals: u8) -> u64 {
        parse_token_amount(amount, decimals).unwrap()
    }

    #[test_case("", 2 => "invalid amount " ; "empty")]
    #[test_case(".", 2 => "invalid amount ." ; "only a point")]
    #[test_case("-1", 2 => "invalid amount -1")]
    #[test_case("1.2.3", 2 => "invalid amount 1.2.3")]
    #[test_case("1e3", 2 => "invalid amount 1e3")]
    #[test_case("1.5", 0 => "amount 1.5 has more than 0 decimal places")]
    #[test_case("0.001", 2 => "amount 0.001 has more than 2 decimal places")]
    #[test_case("18446744073709551616", 0 => "amount 18446744073709551616 is too large")]
    fn rejects_invalid_token_amounts(amount: &str, decimals: u8) -> String {
        parse_token_amount(amount, decimals)
            .unwrap_err()
            .to_string()
    }
}
