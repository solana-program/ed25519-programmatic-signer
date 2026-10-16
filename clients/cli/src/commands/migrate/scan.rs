use {
    super::{
        mint::AuthorityType, new_owner_programmatic_signer, stake::role_name,
        vote::authorized_withdrawer,
    },
    crate::{
        client::{
            Client, decode_mint, decode_stake_account, decode_upgradeable_loader_account,
            decode_vote_account,
        },
        output::OutputFormat,
    },
    anyhow::{Context, Result, ensure},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_account_decoder_client_types::{UiDataSliceConfig, token::real_number_string_trimmed},
    solana_address::Address,
    solana_loader_v3_interface::state::UpgradeableLoaderState,
    solana_native_token::Sol,
    solana_program_pack::Pack,
    solana_rpc_client_types::filter::{Memcmp, RpcFilterType},
    solana_stake_interface::state::{StakeAuthorize, StakeStateV2},
    spl_associated_token_account_interface::address::get_associated_token_address_with_program_id,
    spl_token_2022_interface::{
        extension::AccountType,
        state::{Account as TokenAccount, Mint},
    },
    std::{
        collections::BTreeMap,
        fmt::{self, Write},
        str::FromStr,
    },
};

// Stake account layout: a u32 state tag and a u64 rent-exempt reserve, then the staker and
// withdrawer.
const STAKER_OFFSET: usize = 12;
const WITHDRAWER_OFFSET: usize = 44;
// Vote account layout: a u32 version tag and the node identity, then the withdrawer.
const VOTE_WITHDRAWER_OFFSET: usize = 36;
// Program data layout: a u32 state tag and a u64 slot, then the upgrade authority as an option.
const PROGRAM_DATA_TAG: [u8; 4] = [3, 0, 0, 0];
const UPGRADE_AUTHORITY_OPTION_OFFSET: usize = 12;
const UPGRADE_AUTHORITY_OFFSET: usize = 13;
// Program layout: a u32 state tag, then the program data address.
const PROGRAM_TAG: [u8; 4] = [2, 0, 0, 0];
const PROGRAM_DATA_ADDRESS_OFFSET: usize = 4;
// Mint layout: the mint authority after its u32 option tag, then the supply, decimals and
// initialized flag, then the freeze authority after its u32 option tag.
const MINT_AUTHORITY_OFFSET: usize = 4;
const FREEZE_AUTHORITY_OFFSET: usize = 50;

#[derive(Debug, Args)]
pub(crate) struct ScanCommand {
    /// Address to list migratable accounts and balances for: the authority of stake accounts, vote
    /// accounts, programs and mints, and the owner of token accounts and SOL.
    owner: Address,

    /// Signing pubkey of the new authority, filled into the suggested migrate commands. Pass the
    /// signing pubkey itself, not its programmatic signer. When omitted, the commands default to
    /// moving to the owner's own programmatic signer.
    #[clap(long)]
    new_authority_signing_pubkey: Option<Address>,
}

pub(super) async fn run(
    command: ScanCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let owner = &command.owner;
    let new_authority_signing_pubkey = match &command.new_authority_signing_pubkey {
        Some(new_authority_signing_pubkey) => {
            let new_owner_programmatic_signer =
                new_owner_programmatic_signer(new_authority_signing_pubkey)?;
            ensure!(
                *owner != new_owner_programmatic_signer,
                "owner {owner} is already the new authority's programmatic signer"
            );
            Some(new_authority_signing_pubkey.to_string())
        }
        // The migrate commands default to the owner, which only works if it is on curve.
        None if owner.is_on_curve() => None,
        None => Some("<NEW_AUTHORITY_SIGNING_PUBKEY>".to_string()),
    };
    let new_authority_args = match &new_authority_signing_pubkey {
        Some(new_authority_signing_pubkey) => {
            vec![
                "--new-authority-signing-pubkey",
                new_authority_signing_pubkey,
            ]
        }
        None => vec![],
    };

    // Each category is scanned and reported on its own, some RPC nodes may be limited
    // for some categories
    let (stake, vote, program, mint, token, sol) = tokio::join!(
        scan_stake(client, owner, &new_authority_args),
        scan_vote(client, owner, &new_authority_args),
        scan_program(client, owner, &new_authority_args),
        scan_mint(client, owner, &new_authority_args),
        scan_token(client, owner, &new_authority_args),
        scan_sol(client, owner, &new_authority_args),
    );
    let mut errors = vec![];
    output.render(&ScanOutput {
        owner: owner.to_string(),
        stake: or_record_error("stake", stake, &mut errors),
        vote: or_record_error("vote", vote, &mut errors),
        program: or_record_error("program", program, &mut errors),
        mint: or_record_error("mint", mint, &mut errors),
        token: or_record_error("token", token, &mut errors),
        sol: or_record_error("sol", sol, &mut errors).flatten(),
        errors,
    })
}

/// The scan result, or none when the scan failed, recording the failure.
fn or_record_error<T>(category: &str, result: Result<T>, errors: &mut Vec<ScanError>) -> Option<T> {
    result
        .map_err(|error| {
            errors.push(ScanError {
                category: category.to_string(),
                error: format!("{error:#}"),
            });
        })
        .ok()
}

fn migrate_command(args: &[&str]) -> String {
    format!("spl-programmatic-signer migrate {}", args.join(" "))
}

fn memcmp(offset: usize, bytes: &[u8]) -> RpcFilterType {
    RpcFilterType::Memcmp(Memcmp::new_base58_encoded(offset, bytes))
}

fn data_size(size: usize) -> RpcFilterType {
    RpcFilterType::DataSize(size as u64)
}

async fn scan_stake(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Vec<StakeScanItem>> {
    let mut accounts = BTreeMap::<Address, (u64, Vec<StakeAuthorize>)>::new();
    for (role, offset) in [
        (StakeAuthorize::Staker, STAKER_OFFSET),
        (StakeAuthorize::Withdrawer, WITHDRAWER_OFFSET),
    ] {
        let matches = client
            .program_accounts(
                &solana_stake_interface::program::id(),
                vec![
                    data_size(StakeStateV2::size_of()),
                    memcmp(offset, owner.as_ref()),
                ],
                None,
            )
            .await?;
        for (address, account) in matches {
            let Some(meta) = decode_stake_account(&address, &account)
                .ok()
                .and_then(|state| state.meta())
            else {
                continue;
            };
            let authority = match role {
                StakeAuthorize::Staker => meta.authorized.staker,
                StakeAuthorize::Withdrawer => meta.authorized.withdrawer,
            };
            if authority == *owner {
                accounts
                    .entry(address)
                    .or_insert((account.lamports, vec![]))
                    .1
                    .push(role);
            }
        }
    }

    Ok(accounts
        .into_iter()
        .map(|(address, (lamports, roles))| {
            let address = address.to_string();
            let mut args = vec!["stake", &address];
            args.extend(new_authority_args);
            // `migrate stake` moves both roles by default
            // if `roles` is only one, we add `--authority-type` for it
            // note that we only add an entry with a role, so this is always non-empty
            if let [role] = roles.as_slice() {
                args.extend(["--authority-type", role_name(*role)]);
            }
            StakeScanItem {
                command: migrate_command(&args),
                authority_types: roles
                    .iter()
                    .map(|role| role_name(*role).to_string())
                    .collect(),
                lamports,
                stake_account: address,
            }
        })
        .collect())
}

async fn scan_vote(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Vec<VoteScanItem>> {
    let matches = client
        .program_accounts(
            &solana_sdk_ids::vote::id(),
            vec![memcmp(VOTE_WITHDRAWER_OFFSET, owner.as_ref())],
            None,
        )
        .await?;
    Ok(matches
        .into_iter()
        .filter(|(address, account)| {
            decode_vote_account(address, account)
                .and_then(|state| authorized_withdrawer(&state, address))
                .is_ok_and(|withdrawer| withdrawer == *owner)
        })
        .map(|(address, account)| {
            let address = address.to_string();
            VoteScanItem {
                command: migrate_command(&[&["vote", &address], new_authority_args].concat()),
                lamports: account.lamports,
                vote_account: address,
            }
        })
        .collect())
}

async fn scan_program(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Vec<ProgramScanItem>> {
    let loader = solana_sdk_ids::bpf_loader_upgradeable::id();
    let program_data_accounts = client
        .program_accounts(
            &loader,
            vec![
                memcmp(0, &PROGRAM_DATA_TAG),
                memcmp(UPGRADE_AUTHORITY_OPTION_OFFSET, &[1]),
                memcmp(UPGRADE_AUTHORITY_OFFSET, owner.as_ref()),
            ],
            // Only fetch the program data header, not the ELF that follows it.
            Some(UiDataSliceConfig {
                offset: 0,
                length: UpgradeableLoaderState::size_of_programdata_metadata(),
            }),
        )
        .await?;

    let mut items = vec![];
    for (program_data, account) in program_data_accounts {
        let Ok(UpgradeableLoaderState::ProgramData {
            upgrade_authority_address: Some(upgrade_authority),
            ..
        }) = decode_upgradeable_loader_account(&program_data, &account)
        else {
            continue;
        };
        if upgrade_authority != *owner {
            continue;
        }
        // `migrate program` takes the program address, which can't be derived from the
        // program data address, so find the program account that points to it.
        let programs = client
            .program_accounts(
                &loader,
                vec![
                    data_size(UpgradeableLoaderState::size_of_program()),
                    memcmp(0, &PROGRAM_TAG),
                    memcmp(PROGRAM_DATA_ADDRESS_OFFSET, program_data.as_ref()),
                ],
                None,
            )
            .await?;
        for (program, _) in programs {
            let program = program.to_string();
            items.push(ProgramScanItem {
                command: migrate_command(&[&["program", &program], new_authority_args].concat()),
                program,
                program_data: program_data.to_string(),
            });
        }
    }
    Ok(items)
}

async fn scan_mint(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Vec<MintScanItem>> {
    let token_2022 = spl_token_2022_interface::id();
    let base_mint = data_size(Mint::LEN);
    // Token-2022 mints with extensions are padded to the length of a token account, followed
    // by their account type.
    let extended_mint = memcmp(TokenAccount::LEN, &[AccountType::Mint as u8]);
    let mut queries = vec![];
    for (token_program, layout) in [
        (spl_token_2022_interface::inline_spl_token::id(), &base_mint),
        (token_2022, &base_mint),
        (token_2022, &extended_mint),
    ] {
        for (role, offset) in [
            (AuthorityType::Mint, MINT_AUTHORITY_OFFSET),
            (AuthorityType::Freeze, FREEZE_AUTHORITY_OFFSET),
        ] {
            queries.push((
                token_program,
                role,
                vec![layout.clone(), memcmp(offset, owner.as_ref())],
            ));
        }
    }

    let mut mints = BTreeMap::<Address, (Address, Vec<AuthorityType>)>::new();
    for (token_program, role, filters) in queries {
        let matches = client
            .program_accounts(
                &token_program,
                filters,
                // Skip any extensions, only the base mint is needed.
                Some(UiDataSliceConfig {
                    offset: 0,
                    length: Mint::LEN,
                }),
            )
            .await?;
        for (address, account) in matches {
            let Ok((_, mint)) = decode_mint(&address, &account) else {
                continue;
            };
            let authority: Option<Address> = match role {
                AuthorityType::Mint => mint.mint_authority.into(),
                AuthorityType::Freeze => mint.freeze_authority.into(),
            };
            if authority == Some(*owner) {
                mints
                    .entry(address)
                    .or_insert((token_program, vec![]))
                    .1
                    .push(role);
            }
        }
    }

    Ok(mints
        .into_iter()
        .map(|(address, (token_program, roles))| {
            let address = address.to_string();
            let mut args = vec!["mint", &address];
            args.extend(new_authority_args);
            // only include --authority-type if there is one role
            if let [role] = roles.as_slice() {
                args.extend(["--authority-type", role.name()]);
            }
            MintScanItem {
                command: migrate_command(&args),
                token_program: token_program.to_string(),
                authority_types: roles.iter().map(|role| role.name().to_string()).collect(),
                mint: address,
            }
        })
        .collect())
}

async fn scan_token(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Vec<TokenScanItem>> {
    let owner_arg = owner.to_string();
    let mut items = vec![];
    for token_program in [
        spl_token_2022_interface::inline_spl_token::id(),
        spl_token_2022_interface::id(),
    ] {
        for (address, account) in client
            .token_accounts_by_owner(owner, &token_program)
            .await?
        {
            let amount = u64::from_str(&account.token_amount.amount)
                .with_context(|| format!("token account {address} has an invalid amount"))?;
            if amount == 0 {
                continue;
            }
            let mint = Address::from_str(&account.mint)
                .with_context(|| format!("token account {address} has an invalid mint"))?;
            // `migrate token` only moves from the owner's associated token account.
            let associated =
                get_associated_token_address_with_program_id(owner, &mint, &token_program)
                    == address;
            items.push(TokenScanItem {
                command: associated.then(|| {
                    migrate_command(
                        &[
                            &["token", &account.mint, "--owner", &owner_arg],
                            new_authority_args,
                        ]
                        .concat(),
                    )
                }),
                token_account: address.to_string(),
                mint: account.mint,
                token_program: token_program.to_string(),
                // The raw balance, which `migrate token --amount` takes. The RPC node's UI amount
                // may apply an interest or scaling multiplier.
                amount: real_number_string_trimmed(amount, account.token_amount.decimals),
                associated,
            });
        }
    }
    Ok(items)
}

async fn scan_sol(
    client: &Client,
    owner: &Address,
    new_authority_args: &[&str],
) -> Result<Option<SolScanItem>> {
    let Some(account) = client.optional_account(owner).await? else {
        return Ok(None);
    };
    // The System Program can only transfer from its own accounts that hold no data.
    if account.owner != solana_sdk_ids::system_program::id()
        || !account.data.is_empty()
        || account.lamports == 0
    {
        return Ok(None);
    }
    Ok(Some(SolScanItem {
        lamports: account.lamports,
        command: migrate_command(&[&["sol", &owner.to_string()], new_authority_args].concat()),
    }))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutput {
    pub owner: String,
    pub stake: Option<Vec<StakeScanItem>>,
    pub vote: Option<Vec<VoteScanItem>>,
    pub program: Option<Vec<ProgramScanItem>>,
    pub mint: Option<Vec<MintScanItem>>,
    pub token: Option<Vec<TokenScanItem>>,
    pub sol: Option<SolScanItem>,
    /// Categories that could not be scanned, which are null above.
    pub errors: Vec<ScanError>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakeScanItem {
    pub stake_account: String,
    pub lamports: u64,
    /// The stake authorities the owner holds.
    pub authority_types: Vec<String>,
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteScanItem {
    pub vote_account: String,
    pub lamports: u64,
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramScanItem {
    pub program: String,
    pub program_data: String,
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MintScanItem {
    pub mint: String,
    pub token_program: String,
    /// The mint authorities the owner holds.
    pub authority_types: Vec<String>,
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenScanItem {
    pub token_account: String,
    pub mint: String,
    pub token_program: String,
    pub amount: String,
    /// Whether this is the owner's associated token account, the only kind `migrate token`
    /// moves from.
    pub associated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolScanItem {
    pub lamports: u64,
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub category: String,
    pub error: String,
}

impl fmt::Display for ScanOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        writeln!(out, "Owner: {}", self.owner)?;
        let scanned = |category: &str| !self.errors.iter().any(|error| error.category == category);

        if let Some(stake) = &self.stake {
            writeln!(out)?;
            write_heading(&mut out, "Stake accounts", stake.len())?;
            for item in stake {
                writeln!(
                    out,
                    "  {} ({}), {}",
                    item.stake_account,
                    Sol(item.lamports),
                    item.authority_types.join(", ")
                )?;
                writeln!(out, "    {}", item.command)?;
            }
        }
        if let Some(vote) = &self.vote {
            writeln!(out)?;
            write_heading(&mut out, "Vote accounts (withdrawer)", vote.len())?;
            for item in vote {
                writeln!(out, "  {} ({})", item.vote_account, Sol(item.lamports))?;
                writeln!(out, "    {}", item.command)?;
            }
        }
        if let Some(program) = &self.program {
            writeln!(out)?;
            write_heading(&mut out, "Programs (upgrade authority)", program.len())?;
            for item in program {
                writeln!(
                    out,
                    "  {} (program data: {})",
                    item.program, item.program_data
                )?;
                writeln!(out, "    {}", item.command)?;
            }
        }
        if let Some(mint) = &self.mint {
            writeln!(out)?;
            write_heading(&mut out, "Mints", mint.len())?;
            for item in mint {
                writeln!(out, "  {}, {}", item.mint, item.authority_types.join(", "))?;
                writeln!(out, "    {}", item.command)?;
            }
        }
        if let Some(token) = &self.token {
            writeln!(out)?;
            write_heading(&mut out, "Token balances", token.len())?;
            for item in token {
                writeln!(out, "  {} of mint {}", item.amount, item.mint)?;
                match &item.command {
                    Some(command) => writeln!(out, "    {command}")?,
                    None => writeln!(
                        out,
                        "    In {}, not an associated token account, which `migrate token` \
                         doesn't support",
                        item.token_account
                    )?,
                }
            }
        }
        if scanned("sol") {
            writeln!(out)?;
            match &self.sol {
                Some(item) => {
                    writeln!(out, "SOL: {}", Sol(item.lamports))?;
                    writeln!(out, "    {}", item.command)?;
                }
                None => writeln!(out, "SOL: none")?,
            }
        }

        if !self.errors.is_empty() {
            writeln!(out)?;
            writeln!(out, "Not scanned, so there may be more:")?;
            for error in &self.errors {
                writeln!(out, "  {}: {}", error.category, error.error)?;
            }
        }
        f.write_str(out.trim_end())
    }
}

fn write_heading(out: &mut String, heading: &str, count: usize) -> fmt::Result {
    if count == 0 {
        writeln!(out, "{heading}: none")
    } else {
        writeln!(out, "{heading} ({count}):")
    }
}
