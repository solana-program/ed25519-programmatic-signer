use {
    super::{
        decode::read_execution_message,
        state_changes::{AccountChange, AuthorityChange, state_changes},
        v1_transaction::{
            ComputeBudget, compile_message, ensure_transaction_size, unsigned_transaction,
        },
    },
    crate::{
        client::{Client, SimulateConfig},
        output::OutputFormat,
    },
    anyhow::{Context, Result, bail, ensure},
    clap::Args,
    serde::Serialize,
    serde_json::json,
    solana_account::Account,
    solana_account_decoder::{
        encode_ui_account,
        parse_account_data::{AccountAdditionalDataV3, SplTokenAdditionalDataV2},
    },
    solana_account_decoder_client_types::{UiAccountEncoding, token::real_number_string_trimmed},
    solana_address::Address,
    solana_clock::{Clock, UnixTimestamp},
    solana_hash::Hash,
    solana_message::{VersionedMessage, v1},
    solana_native_token::Sol,
    solana_rpc_client_types::response::{RpcSimulateTransactionResult, UiTokenAmount},
    solana_transaction::versioned::VersionedTransaction,
    solana_transaction_status::{UiTransactionTokenBalance, option_serializer::OptionSerializer},
    spl_ed25519_signer_client::{
        ProgrammaticSigner, instruction::submit, message::authorization_message,
    },
    spl_message_executor_client::instruction::execute,
    spl_token_2022_interface::{
        extension::{
            BaseStateWithExtensions, StateWithExtensions,
            interest_bearing_mint::InterestBearingConfig, scaled_ui_amount::ScaledUiAmountConfig,
        },
        state::Mint,
    },
    std::{
        collections::{BTreeMap, BTreeSet, HashSet},
        fmt,
    },
};

#[derive(Debug, Args)]
pub(super) struct SimulateCommand {
    /// Base64-encoded v1 execution message.
    execution_message: String,

    /// SPL nonce account protecting this execution.
    nonce_account: Address,

    /// Nonce account authority. Defaults to the nonce account's authority.
    #[clap(long)]
    nonce_authority: Option<Address>,

    /// Expected nonce value, which replaces the execution message's recent blockhash.
    /// Defaults to the nonce account's current value.
    #[clap(long)]
    nonce_hash: Option<Hash>,

    /// Remove transaction config fields (such as compute unit limit and priority fee) from the
    /// execution message instead of rejecting it. These fields only apply to top-level
    /// transactions and are ignored by the executor.
    #[clap(long)]
    strip_transaction_config: bool,

    /// Signing pubkey whose programmatic signer (PDA) to label in the changes. Repeat for each
    /// signing pubkey. Signing pubkeys that are accounts of the transaction are labeled automatically. Only affects
    /// the output.
    #[clap(long)]
    signing_pubkey: Vec<Address>,

    /// Also print the full simulation result to stderr, including logs, inner instructions and
    /// the post-simulation state of every writable account.
    #[clap(long)]
    verbose: bool,
}

pub(super) async fn run(
    command: SimulateCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let nonce = client.nonce_account(&command.nonce_account).await?.state;
    let nonce_authority = command.nonce_authority.unwrap_or(nonce.authority);
    let nonce_hash = command.nonce_hash.unwrap_or(nonce.nonce);
    let mut execution_message =
        read_execution_message(&command.execution_message, command.strip_transaction_config)?;
    execution_message.lifetime_specifier = nonce_hash;
    ensure!(
        nonce_hash == nonce.nonce,
        "expected nonce value {nonce_hash}, but nonce account {} currently has {}",
        command.nonce_account,
        nonce.nonce
    );
    ensure!(
        nonce_authority == nonce.authority,
        "expected nonce authority {nonce_authority}, but nonce account {} has authority {}",
        command.nonce_account,
        nonce.authority
    );
    // The fee payer only needs to be funded. Signatures are not verified.
    let fee_payer = client.fee_payer()?.try_pubkey()?;

    // Simulate the relay transaction's Submit -> Execute path, so the message instructions run at
    // the CPI depth they will have onchain.
    // We make each signer (including programmatic signers) a relay signer, so all authorization message signatures are missing
    // This means that `sigVerify=false` works for all signers, and we can simulate the full flow
    let instruction = execute(&command.nonce_account, &nonce_authority, &execution_message);
    let relay_signers = instruction
        .accounts
        .iter()
        .filter(|meta| meta.is_signer)
        .map(|meta| meta.pubkey)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ensure!(
        relay_signers.len() <= usize::from(v1::MAX_SIGNATURES),
        "too many execution signers to simulate: {} exceeds the v1 limit of {}",
        relay_signers.len(),
        v1::MAX_SIGNATURES
    );
    let authorization_message = authorization_message(&instruction, &relay_signers)
        .context("invalid authorization message")?;
    let mut instruction = submit(
        vec![None; relay_signers.len()],
        VersionedMessage::V1(authorization_message),
    );
    // The authorization message's required signers lead its account keys, which the Submit
    // accounts mirror.
    for meta in &mut instruction.accounts[..relay_signers.len()] {
        meta.is_signer = true;
    }
    // Simulate replaces the blockhash.
    let message = compile_message(
        &fee_payer,
        &[instruction],
        Hash::default(),
        ComputeBudget::default(),
    )?;
    ensure_transaction_size("simulated transaction", &message)?;
    // Signatures are not verified.
    let transaction = unsigned_transaction(message.clone());

    output.render(
        &simulate(
            client,
            &transaction,
            &message,
            &SimulateOptions {
                signing_pubkeys: &command.signing_pubkey,
                relay: false,
                replace_recent_blockhash: true,
                verbose: command.verbose,
            },
        )
        .await?,
    )
}

pub(super) struct SimulateOptions<'a> {
    /// Authorities whose derived PDA signers to label in the changes.
    pub(super) signing_pubkeys: &'a [Address],
    /// The simulated message is the relay transaction itself, so the fee is included.
    pub(super) relay: bool,
    /// Replace the transaction's blockhash. A durable nonce transaction must keep its nonce value.
    pub(super) replace_recent_blockhash: bool,
    /// Also print the full simulation result to stderr.
    pub(super) verbose: bool,
}

/// Simulate `transaction`, whose message is `message`, and summarize it, or fail with its logs.
pub(super) async fn simulate(
    client: &Client,
    transaction: &VersionedTransaction,
    message: &v1::Message,
    options: &SimulateOptions<'_>,
) -> Result<SimulateOutput> {
    let addresses = writable_accounts(message);
    // Simulate at the slot of the pre-state or later. The state can still change in between.
    let (slot, pre_accounts) = client.multiple_accounts(&addresses).await?;
    let result = client
        .simulate_transaction(
            transaction,
            &SimulateConfig {
                accounts: &addresses,
                min_context_slot: Some(slot),
                inner_instructions: options.verbose,
                replace_recent_blockhash: options.replace_recent_blockhash,
            },
        )
        .await?;
    let post_accounts = post_accounts(&result, &addresses)?;
    if options.verbose {
        let token_data =
            token_additional_data(client, message, &result, &addresses, &post_accounts).await?;
        let result = verbose_result(&result, &addresses, &post_accounts, &token_data)?;
        eprintln!("Simulation result:\n{result}\n");
    }
    // After the verbose result, so it's printed for a failed simulation too.
    if let Some(error) = &result.err {
        if options.verbose {
            bail!("simulation failed: {error}\nSee the logs in the simulation result above.");
        }
        let logs = result.logs.as_deref().unwrap_or_default().join("\n");
        bail!("simulation failed: {error}\nLogs:\n{logs}");
    }
    let fee = result.fee.unwrap_or(0);
    // Also label the programmatic signers of the transaction's accounts, such as a current
    // authority that moves to its own programmatic signer.
    let signing_pubkeys =
        pda_signing_pubkeys(message.account_keys.iter().chain(options.signing_pubkeys));
    let (account_changes, authority_changes) =
        state_changes(&addresses, &pre_accounts, &post_accounts, &signing_pubkeys);
    Ok(SimulateOutput {
        units_consumed: result.units_consumed,
        fee: options.relay.then_some(fee),
        sol_balance_changes: sol_balance_changes(
            message,
            &result,
            if options.relay { 0 } else { fee },
            &signing_pubkeys,
        )?,
        token_balance_changes: token_balance_changes(message, &result, &signing_pubkeys)?,
        account_changes,
        authority_changes,
    })
}

/// Map the programmatic signer (PDA) of each of `signing_pubkeys` back to its signing pubkey, to
/// label the changes.
fn pda_signing_pubkeys<'a>(
    signing_pubkeys: impl IntoIterator<Item = &'a Address>,
) -> BTreeMap<String, String> {
    signing_pubkeys
        .into_iter()
        .map(|signing_pubkey| {
            let pda = ProgrammaticSigner::derive_address(
                &spl_ed25519_signer_client::id(),
                signing_pubkey,
            );
            (pda.to_string(), signing_pubkey.to_string())
        })
        .collect()
}

/// Accounts the message may write to.
fn writable_accounts(message: &v1::Message) -> Vec<Address> {
    message
        .account_keys
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            message.is_maybe_writable_with_reserved_addresses(*index, None::<&HashSet<Address>>)
        })
        .map(|(_, address)| *address)
        .collect()
}

/// The post-simulation state of `addresses`, decoded from the simulation result.
fn post_accounts(
    result: &RpcSimulateTransactionResult,
    addresses: &[Address],
) -> Result<Vec<Option<Account>>> {
    if result.err.is_some() {
        // A failed simulation returns no accounts. `simulate` reports the error.
        return Ok(vec![None; addresses.len()]);
    }
    let accounts = result
        .accounts
        .as_ref()
        .context("the RPC node did not return simulated accounts")?;
    ensure!(
        accounts.len() == addresses.len(),
        "the RPC node returned accounts that do not match the requested accounts"
    );
    accounts
        .iter()
        .map(|account| {
            account
                .as_ref()
                .map(|account| {
                    account
                        .to_account()
                        .context("the RPC node returned an invalid simulated account")
                })
                .transpose()
        })
        .collect()
}

/// The full simulation result as pretty JSON, with each returned account parsed if possible and
/// paired with its address, since the RPC returns them by position only. `token_data` is the
/// data needed to parse each token account.
fn verbose_result(
    result: &RpcSimulateTransactionResult,
    addresses: &[Address],
    accounts: &[Option<Account>],
    token_data: &BTreeMap<Address, SplTokenAdditionalDataV2>,
) -> Result<String> {
    let mut value = serde_json::to_value(result).context("failed to encode JSON")?;
    if result.accounts.is_some() {
        value["accounts"] = addresses
            .iter()
            .zip(accounts)
            .map(|(address, account)| {
                // Without it, `encode_ui_account` falls back to base64 for token accounts.
                let additional_data =
                    token_data
                        .get(address)
                        .map(|&token_data| AccountAdditionalDataV3 {
                            spl_token_additional_data: Some(token_data),
                        });
                let account = account.as_ref().map(|account| {
                    encode_ui_account(
                        address,
                        account,
                        UiAccountEncoding::JsonParsed,
                        additional_data,
                        None,
                    )
                });
                json!({ "address": address.to_string(), "account": account })
            })
            .collect();
    }
    serde_json::to_string_pretty(&value).context("failed to encode JSON")
}

/// The data needed to parse each token account in `message`, as the RPC node would: its mint's
/// decimals, from the simulated token balances, and for Token-2022, the mint's configs that
/// adjust the displayed amount, with the clock's timestamp. If fetching those fails, warns and
/// leaves them out, so the simulation result still prints.
async fn token_additional_data(
    client: &Client,
    message: &v1::Message,
    result: &RpcSimulateTransactionResult,
    addresses: &[Address],
    post_accounts: &[Option<Account>],
) -> Result<BTreeMap<Address, SplTokenAdditionalDataV2>> {
    // The mint and decimals of every token account, for both Token and Token-2022, and the
    // Token-2022 mints, whose display configs need fetching.
    let mut token_accounts = BTreeMap::new();
    let mut token_2022_mints = BTreeSet::new();
    let token_2022 = spl_token_2022_interface::id().to_string();
    for balance in result
        .pre_token_balances
        .iter()
        .chain(&result.post_token_balances)
        .flatten()
    {
        let Some(address) = message.account_keys.get(usize::from(balance.account_index)) else {
            continue;
        };
        let mint = balance
            .mint
            .parse::<Address>()
            .context("the RPC node returned an invalid mint")?;
        if matches!(
            &balance.program_id,
            OptionSerializer::Some(program_id) if program_id == &token_2022
        ) {
            token_2022_mints.insert(mint);
        }
        token_accounts.insert(*address, (mint, balance.ui_token_amount.decimals));
    }

    let (configs, timestamp) = if token_2022_mints.is_empty() {
        (BTreeMap::new(), 0)
    } else {
        let mints = token_2022_mints.into_iter().collect::<Vec<_>>();
        token_2022_configs(client, &mints, addresses, post_accounts)
            .await
            .unwrap_or_else(|error| {
                eprintln!(
                    "Warning: token amounts are not adjusted for interest-bearing or scaled UI \
                     amount mints: {error:#}"
                );
                (BTreeMap::new(), 0)
            })
    };

    Ok(token_accounts
        .into_iter()
        .map(|(address, (mint, decimals))| {
            let (interest_bearing, scaled_ui_amount) =
                configs.get(&mint).copied().unwrap_or_default();
            let token_data = SplTokenAdditionalDataV2 {
                decimals,
                interest_bearing_config: interest_bearing.map(|config| (config, timestamp)),
                scaled_ui_amount_config: scaled_ui_amount.map(|config| (config, timestamp)),
            };
            (address, token_data)
        })
        .collect())
}

/// A Token-2022 mint's configs that adjust its displayed token amounts.
type MintDisplayConfigs = (Option<InterestBearingConfig>, Option<ScaledUiAmountConfig>);

/// The display configs of each of the Token-2022 `mints` that adjust token amounts, with the
/// clock's timestamp. Fetches the mints and the clock, which are read before the simulation,
/// except for mints in `addresses`, whose post-simulation state is in `post_accounts`.
async fn token_2022_configs(
    client: &Client,
    mints: &[Address],
    addresses: &[Address],
    post_accounts: &[Option<Account>],
) -> Result<(BTreeMap<Address, MintDisplayConfigs>, UnixTimestamp)> {
    let clock_address = solana_sdk_ids::sysvar::clock::id();
    let (_, accounts) = client
        .multiple_accounts(&[mints, &[clock_address]].concat())
        .await?;
    let (clock, mint_accounts) = accounts
        .split_last()
        .context("the RPC node returned no accounts")?;
    let clock = clock
        .as_ref()
        .with_context(|| format!("the clock sysvar {clock_address} was not found"))?;
    let timestamp = wincode::deserialize::<Clock>(&clock.data)
        .context("invalid clock sysvar data")?
        .unix_timestamp;
    let mut configs = BTreeMap::new();
    for (mint, account) in mints.iter().zip(mint_accounts) {
        // Prefer the post-simulation state of a mint the transaction writes to.
        let account = addresses
            .iter()
            .position(|address| address == mint)
            .and_then(|index| post_accounts[index].as_ref())
            .or(account.as_ref());
        // Unparsed mints just leave their accounts without the configs.
        let Some(state) =
            account.and_then(|account| StateWithExtensions::<Mint>::unpack(&account.data).ok())
        else {
            continue;
        };
        configs.insert(
            *mint,
            (
                state.get_extension::<InterestBearingConfig>().ok().copied(),
                state.get_extension::<ScaledUiAmountConfig>().ok().copied(),
            ),
        );
    }
    Ok((configs, timestamp))
}

/// Lamport changes per account, with `fee` added back to the fee payer. `signing_pubkeys` maps
/// programmatic signers to their signing pubkeys.
fn sol_balance_changes(
    message: &v1::Message,
    result: &RpcSimulateTransactionResult,
    fee: u64,
    signing_pubkeys: &BTreeMap<String, String>,
) -> Result<Vec<SolBalanceChange>> {
    let (Some(pre), Some(post)) = (&result.pre_balances, &result.post_balances) else {
        bail!("the RPC node did not return simulated balances");
    };
    ensure!(
        pre.len() == message.account_keys.len() && post.len() == message.account_keys.len(),
        "the RPC node returned balances that do not match the simulated accounts"
    );
    Ok(message
        .account_keys
        .iter()
        .zip(pre.iter().zip(post))
        .enumerate()
        .filter_map(|(index, (address, (pre, post)))| {
            let fee = if index == 0 { i128::from(fee) } else { 0 };
            let change = i128::from(*post)
                .saturating_sub(i128::from(*pre))
                .saturating_add(fee);
            (change != 0).then(|| {
                let address = address.to_string();
                SolBalanceChange {
                    signing_pubkey: signing_pubkeys.get(&address).cloned(),
                    address,
                    lamports: change,
                }
            })
        })
        .collect())
}

/// Token amount changes per token account. An account missing on one side was created or
/// closed, so its amount there is zero. `signing_pubkeys` maps programmatic signers to their
/// signing pubkeys, matched against the token account owner.
fn token_balance_changes(
    message: &v1::Message,
    result: &RpcSimulateTransactionResult,
    signing_pubkeys: &BTreeMap<String, String>,
) -> Result<Vec<TokenBalanceChange>> {
    let (Some(pre), Some(post)) = (&result.pre_token_balances, &result.post_token_balances) else {
        bail!("the RPC node did not return simulated token balances");
    };
    let mut balances = BTreeMap::<u8, (Option<_>, Option<_>)>::new();
    for balance in pre {
        balances.entry(balance.account_index).or_default().0 = Some(balance);
    }
    for balance in post {
        balances.entry(balance.account_index).or_default().1 = Some(balance);
    }

    let mut changes = Vec::new();
    for (index, (pre, post)) in balances {
        let amount = |balance: Option<&UiTransactionTokenBalance>| -> Result<i128> {
            balance.map_or(Ok(0), |balance| {
                balance
                    .ui_token_amount
                    .amount
                    .parse::<u64>()
                    .map(i128::from)
                    .context("the RPC node returned an invalid token amount")
            })
        };
        let change = amount(post)?.saturating_sub(amount(pre)?);
        if change == 0 {
            continue;
        }
        // Infallible: one side is present for every entry.
        // Note: currently doesn't handle these fields changing between pre/post balances.
        let UiTransactionTokenBalance {
            mint,
            owner,
            program_id,
            ui_token_amount: UiTokenAmount { decimals, .. },
            ..
        } = post.or(pre).unwrap();
        let account = message
            .account_keys
            .get(usize::from(index))
            .context("the RPC node returned a token balance for an unknown account")?;
        let owner = Option::<String>::from(owner.clone());
        changes.push(TokenBalanceChange {
            account: account.to_string(),
            mint: mint.clone(),
            signing_pubkey: owner
                .as_ref()
                .and_then(|owner| signing_pubkeys.get(owner).cloned()),
            owner,
            program_id: Option::from(program_id.clone()),
            amount: change,
            decimals: *decimals,
        });
    }
    Ok(changes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SimulateOutput {
    units_consumed: Option<u64>,
    /// Relay transaction fee, in lamports. Only set when simulating the relay transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    fee: Option<u64>,
    sol_balance_changes: Vec<SolBalanceChange>,
    token_balance_changes: Vec<TokenBalanceChange>,
    /// Accounts created, closed, reassigned or re-sized.
    account_changes: Vec<AccountChange>,
    /// Authority changes to accounts owned by known programs.
    authority_changes: Vec<AuthorityChange>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SolBalanceChange {
    address: String,
    lamports: i128,
    /// Signing pubkey whose programmatic signer is this address, if known. Omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_pubkey: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenBalanceChange {
    account: String,
    mint: String,
    owner: Option<String>,
    program_id: Option<String>,
    /// Raw amount change, in base units.
    amount: i128,
    decimals: u8,
    /// Signing pubkey whose programmatic signer owns this token account, if known. Omitted
    /// otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_pubkey: Option<String>,
}

impl fmt::Display for SimulateOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Simulation succeeded")?;
        match (self.units_consumed, self.fee) {
            (Some(units), Some(_)) => write!(f, " ({units} compute units)")?,
            (Some(units), None) => write!(
                f,
                " ({units} compute units, excluding authority signature verification)"
            )?,
            (None, _) => {}
        }
        writeln!(f)?;
        if let Some(fee) = self.fee {
            writeln!(f, "Fee: {}", Sol(fee))?;
        }

        writeln!(f, "\nSOL balance changes:")?;
        if self.sol_balance_changes.is_empty() {
            writeln!(f, "  none")?;
        }
        for change in &self.sol_balance_changes {
            writeln!(f, "  {change}")?;
        }

        writeln!(f, "\nToken balance changes:")?;
        if self.token_balance_changes.is_empty() {
            writeln!(f, "  none")?;
        }
        for change in &self.token_balance_changes {
            writeln!(f, "  {change}")?;
        }

        writeln!(f, "\nAccount changes:")?;
        if self.account_changes.is_empty() {
            writeln!(f, "  none")?;
        }
        for change in &self.account_changes {
            writeln!(f, "  {change}")?;
        }

        write!(f, "\nAuthority changes:")?;
        if self.authority_changes.is_empty() {
            write!(f, "\n  none")?;
        }
        for change in &self.authority_changes {
            write!(f, "\n  {change}")?;
        }
        Ok(())
    }
}

impl fmt::Display for SolBalanceChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.lamports < 0 { '-' } else { '+' };
        // Infallible: the change of one u64 balance fits in a u64.
        let lamports = u64::try_from(self.lamports.unsigned_abs()).unwrap();
        write!(f, "{}  {sign}{}", self.address, Sol(lamports))?;
        if let Some(signing_pubkey) = &self.signing_pubkey {
            write!(f, "  (PDA of {signing_pubkey})")?;
        }
        Ok(())
    }
}

impl fmt::Display for TokenBalanceChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  {}  mint {}",
            self.account,
            format_token_amount(self.amount, self.decimals),
            self.mint
        )?;
        if let Some(owner) = &self.owner {
            write!(f, "  owner {owner}")?;
        }
        if let Some(signing_pubkey) = &self.signing_pubkey {
            write!(f, "  (PDA of {signing_pubkey})")?;
        }
        Ok(())
    }
}

/// Format a signed raw token amount with its decimals, e.g. `-1.5`.
fn format_token_amount(amount: i128, decimals: u8) -> String {
    let sign = if amount < 0 { '-' } else { '+' };
    // Infallible: the change of one u64 amount fits in a u64.
    let amount = u64::try_from(amount.unsigned_abs()).unwrap();
    format!("{sign}{}", real_number_string_trimmed(amount, decimals))
}

#[cfg(test)]
mod tests {
    use {super::format_token_amount, test_case::test_case};

    #[test_case(0, 0 => "+0")]
    #[test_case(-5, 0 => "-5")]
    #[test_case(1_500_000, 6 => "+1.5")]
    #[test_case(-1, 6 => "-0.000001")]
    #[test_case(2_000_000, 6 => "+2")]
    fn formats_token_amounts(amount: i128, decimals: u8) -> String {
        format_token_amount(amount, decimals)
    }
}
