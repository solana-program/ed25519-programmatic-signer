//! Account and authority changes between the pre- and post-simulation state of the accounts a
//! transaction writes to.

use {
    crate::mint_authority::{self, AuthorityType as MintAuthorityType},
    serde::Serialize,
    solana_account::Account,
    solana_account_decoder::{
        parse_account_data::SplTokenAdditionalDataV2, parse_token::token_amount_to_ui_amount_v3,
    },
    solana_address::Address,
    solana_clock::Epoch,
    solana_loader_v3_interface::state::UpgradeableLoaderState,
    solana_nonce::{state::State as NonceState, versions::Versions as NonceVersions},
    solana_stake_interface::state::StakeStateV2,
    solana_vote_interface::state::VoteStateVersions,
    spl_token_2022_interface::{
        check_spl_token_program_account, extension::StateWithExtensions,
        state::Account as TokenAccount,
    },
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt,
    },
};

/// An account that was created, closed, reassigned or re-sized, for any owner program.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountChange {
    address: String,
    /// The account before the transaction. `None` if it was created.
    before: Option<AccountSummary>,
    /// The account after the transaction. `None` if it was closed.
    after: Option<AccountSummary>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSummary {
    owner: String,
    /// Data size, in bytes.
    size: usize,
}

impl AccountSummary {
    fn new(account: &Account) -> Self {
        Self {
            owner: account.owner.to_string(),
            size: account.data.len(),
        }
    }
}

/// A change to one authority of an account owned by a program we can parse.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AuthorityChange {
    account: String,
    kind: AccountKind,
    role: AuthorityRole,
    /// The authority before the transaction. `None` if it was unset, or the account did not exist
    /// or could not be parsed.
    before: Option<AuthorityValue>,
    /// The authority after the transaction. `None` if it is unset, or the account was closed or
    /// could not be parsed.
    after: Option<AuthorityValue>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorityValue {
    address: String,
    /// Serialized as its variant's field, e.g. `effectiveEpoch`. Omitted for an authority with no
    /// relevant limit.
    #[serde(flatten)]
    limit: Option<AuthorityLimit>,
    /// Signing pubkey whose programmatic signer is this address, if known. Omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_pubkey: Option<String>,
}

/// A limit on when or how much an authority may act, for authorities that have one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum AuthorityLimit {
    /// First epoch a vote account's voter applies to, since a new voter takes effect at a later
    /// epoch.
    EffectiveEpoch(Epoch),
    /// Amount a token account's delegate may transfer.
    DelegatedAmount(TokenAmount),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenAmount {
    /// Raw amount, in base units.
    amount: u64,
    /// The mint's decimals, if known. Omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    decimals: Option<u8>,
    /// The amount in tokens, as a wallet displays it: with the mint's decimals, and adjusted for
    /// an interest-bearing or scaled UI amount mint. Omitted if the mint's decimals are unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    ui_amount: Option<String>,
}

impl TokenAmount {
    fn new(amount: u64, token_data: Option<&SplTokenAdditionalDataV2>) -> Self {
        Self {
            amount,
            decimals: token_data.map(|token_data| token_data.decimals),
            ui_amount: token_data
                .map(|token_data| token_amount_to_ui_amount_v3(amount, token_data).ui_amount_string)
                // Empty if the mint's config can't convert the amount.
                .filter(|ui_amount| !ui_amount.is_empty()),
        }
    }
}

/// The kind of a parsed account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum AccountKind {
    Stake,
    Vote,
    /// A Token or Token-2022 account. Extensions are not parsed.
    TokenAccount,
    /// A Token or Token-2022 mint, including the authorities of its Token-2022 extensions.
    Mint,
    /// An upgradeable loader program data account, which holds its program's upgrade authority.
    ProgramData,
    /// An upgradeable loader buffer account.
    Buffer,
    /// A System Program durable nonce account.
    DurableNonce,
}

/// The role of an authority of a parsed account. Changes are listed in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum AuthorityRole {
    Staker,
    /// A vote account's validator identity (node pubkey). Not an authority, but it decides who
    /// runs the validator.
    Identity,
    Withdrawer,
    LockupCustodian,
    Voter,
    InflationRewardsCollector,
    BlockRevenueCollector,
    /// A token account's owner.
    Owner,
    /// A token account's delegate, which may transfer up to its delegated amount.
    Delegate,
    /// A token account's or mint's close authority.
    CloseAuthority,
    MintAuthority,
    FreezeAuthority,
    TransferFeeConfigAuthority,
    /// A mint's authority to withdraw withheld transfer fees.
    WithheldWithdrawAuthority,
    InterestRateAuthority,
    /// A mint's permanent delegate, which may transfer or burn any holder's tokens.
    PermanentDelegate,
    ConfidentialTransferAuthority,
    TransferHookAuthority,
    ConfidentialTransferFeeAuthority,
    MetadataPointerAuthority,
    GroupPointerAuthority,
    GroupMemberPointerAuthority,
    ScaledUiAmountAuthority,
    PauseAuthority,
    PermissionedBurnAuthority,
    /// The update authority of token metadata stored in a mint.
    MetadataUpdateAuthority,
    /// The update authority of a token group stored in a mint.
    GroupUpdateAuthority,
    UpgradeAuthority,
    /// A buffer account's authority, which may write to it, deploy it or close it, or a durable
    /// nonce account's authority, which may advance it, withdraw from it or authorize another.
    Authority,
}

/// An authority, as parsed from account data.
#[derive(Clone, PartialEq, Eq)]
struct ParsedAuthority {
    address: Address,
    limit: Option<AuthorityLimit>,
}

impl ParsedAuthority {
    fn new(address: Address) -> Self {
        Self {
            address,
            limit: None,
        }
    }
}

/// The authorities of a parsed account, keyed by role.
struct ParsedAccount {
    kind: AccountKind,
    authorities: BTreeMap<AuthorityRole, ParsedAuthority>,
}

/// Compare the state of `addresses` before and after the simulation. A missing or zero-lamport
/// account does not exist. `pda_signing_pubkeys` maps programmatic signers to their signing
/// pubkeys, and `token_data` maps token accounts to their mint's decimals and display configs, to
/// label authority changes.
pub(super) fn state_changes<'a>(
    addresses: &[Address],
    pre: &'a [Option<Account>],
    post: &'a [Option<Account>],
    pda_signing_pubkeys: &BTreeMap<String, String>,
    token_data: &BTreeMap<Address, SplTokenAdditionalDataV2>,
) -> (Vec<AccountChange>, Vec<AuthorityChange>) {
    let mut account_changes = Vec::new();
    let mut authority_changes = Vec::new();
    for ((address, pre), post) in addresses.iter().zip(pre).zip(post) {
        let exists =
            |account: &'a Option<Account>| account.as_ref().filter(|account| account.lamports > 0);
        let (pre, post) = (exists(pre), exists(post));
        account_changes.extend(account_change(address, pre, post));
        let parse = |account: &Account| parse_account(account, token_data.get(address));
        authority_changes.extend(authority_changes_for(
            address,
            pre.and_then(parse),
            post.and_then(parse),
            pda_signing_pubkeys,
        ));
    }
    (account_changes, authority_changes)
}

fn account_change(
    address: &Address,
    pre: Option<&Account>,
    post: Option<&Account>,
) -> Option<AccountChange> {
    let (before, after) = (pre.map(AccountSummary::new), post.map(AccountSummary::new));
    (before != after).then(|| AccountChange {
        address: address.to_string(),
        before,
        after,
    })
}

fn authority_changes_for(
    address: &Address,
    pre: Option<ParsedAccount>,
    post: Option<ParsedAccount>,
    pda_signing_pubkeys: &BTreeMap<String, String>,
) -> Vec<AuthorityChange> {
    let Some(kind) = post.as_ref().or(pre.as_ref()).map(|parsed| parsed.kind) else {
        return vec![];
    };
    let authorities =
        |parsed: Option<ParsedAccount>| parsed.map(|parsed| parsed.authorities).unwrap_or_default();
    let (pre, post) = (authorities(pre), authorities(post));
    let value = |authority: Option<&ParsedAuthority>| {
        authority.map(|authority| {
            let address = authority.address.to_string();
            AuthorityValue {
                signing_pubkey: pda_signing_pubkeys.get(&address).cloned(),
                address,
                limit: authority.limit.clone(),
            }
        })
    };
    pre.keys()
        .chain(post.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|role| pre.get(*role) != post.get(*role))
        .map(|&role| AuthorityChange {
            account: address.to_string(),
            kind,
            role,
            before: value(pre.get(&role)),
            after: value(post.get(&role)),
        })
        .collect()
}

/// Parse the authorities of an account owned by a known program. `None` if the program is
/// unknown or the data is invalid. `token_data` is a token account's mint decimals and display
/// configs, if known.
fn parse_account(
    account: &Account,
    token_data: Option<&SplTokenAdditionalDataV2>,
) -> Option<ParsedAccount> {
    if account.owner == solana_stake_interface::program::id() {
        parse_stake(account)
    } else if account.owner == solana_sdk_ids::vote::id() {
        parse_vote(account)
    } else if check_spl_token_program_account(&account.owner).is_ok() {
        parse_token(account, token_data)
    } else if account.owner == solana_sdk_ids::bpf_loader_upgradeable::id() {
        parse_upgradeable_loader(account)
    } else if account.owner == solana_system_interface::program::id() {
        parse_durable_nonce(account)
    } else {
        None
    }
}

fn parse_stake(account: &Account) -> Option<ParsedAccount> {
    let state = wincode::deserialize::<StakeStateV2>(&account.data).ok()?;
    let mut authorities = BTreeMap::new();
    if let Some(meta) = state.meta() {
        authorities.insert(
            AuthorityRole::Staker,
            ParsedAuthority::new(meta.authorized.staker),
        );
        authorities.insert(
            AuthorityRole::Withdrawer,
            ParsedAuthority::new(meta.authorized.withdrawer),
        );
        // The default custodian means no custodian.
        if meta.lockup.custodian != Address::default() {
            authorities.insert(
                AuthorityRole::LockupCustodian,
                ParsedAuthority::new(meta.lockup.custodian),
            );
        }
    }
    Some(ParsedAccount {
        kind: AccountKind::Stake,
        authorities,
    })
}

fn parse_vote(account: &Account) -> Option<ParsedAccount> {
    let state = wincode::deserialize::<VoteStateVersions>(&account.data).ok()?;
    let mut authorities = BTreeMap::new();
    let (identity, withdrawer, voters) = match &state {
        VoteStateVersions::Uninitialized => {
            return Some(ParsedAccount {
                kind: AccountKind::Vote,
                authorities,
            });
        }
        VoteStateVersions::V1_14_11(state) => (
            state.node_pubkey,
            state.authorized_withdrawer,
            &state.authorized_voters,
        ),
        VoteStateVersions::V3(state) => (
            state.node_pubkey,
            state.authorized_withdrawer,
            &state.authorized_voters,
        ),
        VoteStateVersions::V4(state) => {
            authorities.insert(
                AuthorityRole::InflationRewardsCollector,
                ParsedAuthority::new(state.inflation_rewards_collector),
            );
            authorities.insert(
                AuthorityRole::BlockRevenueCollector,
                ParsedAuthority::new(state.block_revenue_collector),
            );
            (
                state.node_pubkey,
                state.authorized_withdrawer,
                &state.authorized_voters,
            )
        }
    };
    authorities.insert(AuthorityRole::Identity, ParsedAuthority::new(identity));
    authorities.insert(AuthorityRole::Withdrawer, ParsedAuthority::new(withdrawer));
    // A new voter takes effect at a later epoch, so compare the latest entry rather than the
    // current voter. The Vote Program also purges past entries, which this ignores.
    if let Some((epoch, voter)) = voters.last() {
        authorities.insert(
            AuthorityRole::Voter,
            ParsedAuthority {
                address: *voter,
                limit: Some(AuthorityLimit::EffectiveEpoch(*epoch)),
            },
        );
    }
    Some(ParsedAccount {
        kind: AccountKind::Vote,
        authorities,
    })
}

/// Parse a program data or buffer account. A program account only points to its program data, so
/// it has no authorities.
fn parse_upgradeable_loader(account: &Account) -> Option<ParsedAccount> {
    // Program data and buffer accounts store the program's bytes after this state, which
    // `deserialize` ignores.
    let (kind, role, authority) = match wincode::deserialize(&account.data).ok()? {
        UpgradeableLoaderState::ProgramData {
            upgrade_authority_address,
            ..
        } => (
            AccountKind::ProgramData,
            AuthorityRole::UpgradeAuthority,
            upgrade_authority_address,
        ),
        UpgradeableLoaderState::Buffer { authority_address } => (
            AccountKind::Buffer,
            AuthorityRole::Authority,
            authority_address,
        ),
        UpgradeableLoaderState::Uninitialized | UpgradeableLoaderState::Program { .. } => {
            return None;
        }
    };
    // At most one authority. None means the program is immutable, or the buffer can no longer
    // change.
    let authorities = authority
        .map(|authority| (role, ParsedAuthority::new(authority)))
        .into_iter()
        .collect();
    Some(ParsedAccount { kind, authorities })
}

/// Parse a durable nonce account. Other System Program accounts, such as wallets, have no
/// authorities.
fn parse_durable_nonce(account: &Account) -> Option<ParsedAccount> {
    // Like the System Program, ignore any data after the nonce state, since a nonce account may be
    // larger than it.
    let versions = wincode::deserialize::<NonceVersions>(&account.data).ok()?;
    let NonceState::Initialized(data) = versions.state() else {
        return None;
    };
    Some(ParsedAccount {
        kind: AccountKind::DurableNonce,
        authorities: BTreeMap::from([(
            AuthorityRole::Authority,
            ParsedAuthority::new(data.authority),
        )]),
    })
}

/// Parse a token account or a mint. For token accounts, Token-2022 extensions are ignored because
/// they hold no authorities. For mints, Token-2022 extensions are parsed to find their authorities.
fn parse_token(
    account: &Account,
    token_data: Option<&SplTokenAdditionalDataV2>,
) -> Option<ParsedAccount> {
    let (kind, authorities) =
        if let Ok(state) = StateWithExtensions::<TokenAccount>::unpack(&account.data) {
            let TokenAccount {
                owner,
                delegate,
                delegated_amount,
                close_authority,
                ..
            } = state.base;
            let delegate = Option::from(delegate).map(|address| ParsedAuthority {
                address,
                limit: Some(AuthorityLimit::DelegatedAmount(TokenAmount::new(
                    delegated_amount,
                    token_data,
                ))),
            });
            (
                AccountKind::TokenAccount,
                vec![
                    (AuthorityRole::Owner, Some(ParsedAuthority::new(owner))),
                    (AuthorityRole::Delegate, delegate),
                    (
                        AuthorityRole::CloseAuthority,
                        Option::from(close_authority).map(ParsedAuthority::new),
                    ),
                ],
            )
        } else {
            (
                AccountKind::Mint,
                mint_authority::authorities(&account.data)
                    .ok()?
                    .into_iter()
                    .map(|(role, authority)| (mint_role(role), authority.map(ParsedAuthority::new)))
                    .collect(),
            )
        };
    Some(ParsedAccount {
        kind,
        authorities: authorities
            .into_iter()
            .filter_map(|(role, authority)| Some((role, authority?)))
            .collect(),
    })
}

fn mint_role(role: MintAuthorityType) -> AuthorityRole {
    match role {
        MintAuthorityType::Mint => AuthorityRole::MintAuthority,
        MintAuthorityType::Freeze => AuthorityRole::FreezeAuthority,
        MintAuthorityType::TransferFeeConfig => AuthorityRole::TransferFeeConfigAuthority,
        MintAuthorityType::WithheldWithdraw => AuthorityRole::WithheldWithdrawAuthority,
        MintAuthorityType::CloseMint => AuthorityRole::CloseAuthority,
        MintAuthorityType::InterestRate => AuthorityRole::InterestRateAuthority,
        MintAuthorityType::PermanentDelegate => AuthorityRole::PermanentDelegate,
        MintAuthorityType::ConfidentialTransferMint => AuthorityRole::ConfidentialTransferAuthority,
        MintAuthorityType::TransferHookProgramId => AuthorityRole::TransferHookAuthority,
        MintAuthorityType::ConfidentialTransferFeeConfig => {
            AuthorityRole::ConfidentialTransferFeeAuthority
        }
        MintAuthorityType::MetadataPointer => AuthorityRole::MetadataPointerAuthority,
        MintAuthorityType::GroupPointer => AuthorityRole::GroupPointerAuthority,
        MintAuthorityType::GroupMemberPointer => AuthorityRole::GroupMemberPointerAuthority,
        MintAuthorityType::ScaledUiAmount => AuthorityRole::ScaledUiAmountAuthority,
        MintAuthorityType::Pause => AuthorityRole::PauseAuthority,
        MintAuthorityType::PermissionedBurn => AuthorityRole::PermissionedBurnAuthority,
        MintAuthorityType::MetadataUpdate => AuthorityRole::MetadataUpdateAuthority,
        MintAuthorityType::GroupUpdate => AuthorityRole::GroupUpdateAuthority,
    }
}

impl fmt::Display for AccountChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}  ", self.address)?;
        match (&self.before, &self.after) {
            (None, Some(AccountSummary { owner, size })) => {
                write!(f, "created, owner {owner}, {size} bytes")
            }
            (Some(AccountSummary { owner, .. }), None) => write!(f, "closed, owner {owner}"),
            (Some(before), Some(after)) => {
                let mut changes = Vec::new();
                if before.owner != after.owner {
                    changes.push(format!("owner {} -> {}", before.owner, after.owner));
                }
                if before.size != after.size {
                    changes.push(format!("size {} -> {} bytes", before.size, after.size));
                }
                write!(f, "{}", changes.join(", "))
            }
            // Not constructed: an unchanged account has no entry.
            (None, None) => Ok(()),
        }
    }
}

impl fmt::Display for AuthorityChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = |value: &Option<AuthorityValue>| {
            value
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string)
        };
        write!(
            f,
            "{}  {} {}: {} -> {}",
            self.account,
            self.kind,
            self.role,
            value(&self.before),
            value(&self.after)
        )
    }
}

impl fmt::Display for AccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stake => "stake",
            Self::Vote => "vote",
            Self::TokenAccount => "token account",
            Self::Mint => "mint",
            Self::ProgramData => "program data",
            Self::Buffer => "buffer",
            Self::DurableNonce => "durable nonce",
        })
    }
}

impl fmt::Display for AuthorityRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Staker => "staker",
            Self::Identity => "identity",
            Self::Withdrawer => "withdrawer",
            Self::LockupCustodian => "lockup custodian",
            Self::Voter => "voter",
            Self::InflationRewardsCollector => "inflation rewards collector",
            Self::BlockRevenueCollector => "block revenue collector",
            Self::Owner => "owner",
            Self::Delegate => "delegate",
            Self::CloseAuthority => "close authority",
            Self::MintAuthority => "mint authority",
            Self::FreezeAuthority => "freeze authority",
            Self::TransferFeeConfigAuthority => "transfer fee config authority",
            Self::WithheldWithdrawAuthority => "withheld withdraw authority",
            Self::InterestRateAuthority => "interest rate authority",
            Self::PermanentDelegate => "permanent delegate",
            Self::ConfidentialTransferAuthority => "confidential transfer authority",
            Self::TransferHookAuthority => "transfer hook authority",
            Self::ConfidentialTransferFeeAuthority => "confidential transfer fee authority",
            Self::MetadataPointerAuthority => "metadata pointer authority",
            Self::GroupPointerAuthority => "group pointer authority",
            Self::GroupMemberPointerAuthority => "group member pointer authority",
            Self::ScaledUiAmountAuthority => "scaled UI amount authority",
            Self::PauseAuthority => "pause authority",
            Self::PermissionedBurnAuthority => "permissioned burn authority",
            Self::MetadataUpdateAuthority => "metadata update authority",
            Self::GroupUpdateAuthority => "group update authority",
            Self::UpgradeAuthority => "upgrade authority",
            Self::Authority => "authority",
        })
    }
}

impl fmt::Display for AuthorityValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address)?;
        if let Some(signing_pubkey) = &self.signing_pubkey {
            write!(f, " (PDA of {signing_pubkey})")?;
        }
        match &self.limit {
            Some(AuthorityLimit::EffectiveEpoch(epoch)) => write!(f, " (from epoch {epoch})"),
            Some(AuthorityLimit::DelegatedAmount(TokenAmount {
                ui_amount: Some(ui_amount),
                ..
            })) => write!(f, " (delegated {ui_amount})"),
            Some(AuthorityLimit::DelegatedAmount(TokenAmount {
                amount,
                ui_amount: None,
                ..
            })) => write!(f, " (delegated {amount} base units)"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_hash::Hash,
        solana_nonce::state::{Data as NonceData, DurableNonce},
        solana_program_pack::Pack,
        spl_token_2022_interface::{
            extension::scaled_ui_amount::ScaledUiAmountConfig, state::AccountState,
        },
        test_case::test_case,
    };

    fn token_account(delegate: Address, delegated_amount: u64) -> Option<Account> {
        let state = TokenAccount {
            mint: Address::new_from_array([1; 32]),
            owner: Address::new_from_array([2; 32]),
            delegate: Some(delegate).into(),
            state: AccountState::Initialized,
            delegated_amount,
            ..TokenAccount::default()
        };
        let mut data = vec![0; TokenAccount::LEN];
        state.pack_into_slice(&mut data);
        Some(Account {
            lamports: 1,
            data,
            owner: spl_token_2022_interface::id(),
            ..Account::default()
        })
    }

    /// A change to only the delegated amount, such as the delegate transferring some of it, is a
    /// delegate change.
    #[test_case(Some(2), None, "1.5", "0.5"; "with decimals")]
    #[test_case(Some(2), Some(2.0), "3", "1"; "with a scaled UI amount")]
    #[test_case(None, None, "150 base units", "50 base units"; "without decimals")]
    fn reports_delegated_amount_change(
        decimals: Option<u8>,
        multiplier: Option<f64>,
        before: &str,
        after: &str,
    ) {
        let address = Address::new_from_array([3; 32]);
        let delegate = Address::new_from_array([4; 32]);
        let (_, changes) = state_changes(
            &[address],
            &[token_account(delegate, 150)],
            &[token_account(delegate, 50)],
            &BTreeMap::new(),
            &decimals
                .map(|decimals| {
                    // In effect from timestamp 0.
                    let config = multiplier.map(|multiplier| ScaledUiAmountConfig {
                        new_multiplier: multiplier.into(),
                        ..ScaledUiAmountConfig::default()
                    });
                    let token_data = SplTokenAdditionalDataV2 {
                        decimals,
                        scaled_ui_amount_config: config.map(|config| (config, 0)),
                        ..SplTokenAdditionalDataV2::default()
                    };
                    (address, token_data)
                })
                .into_iter()
                .collect(),
        );
        let changes = changes.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            changes,
            [format!(
                "{address}  token account delegate: {delegate} (delegated {before}) -> {delegate} \
                 (delegated {after})"
            )]
        );
    }

    /// A nonce account larger than the nonce state is still a durable nonce, as for the System
    /// Program.
    #[test]
    fn reports_authority_change_of_larger_nonce_account() {
        let address = Address::new_from_array([1; 32]);
        let nonce_account = |authority: Address| {
            let state = NonceVersions::new(NonceState::Initialized(NonceData::new(
                authority,
                DurableNonce::from_blockhash(&Hash::default()),
                5_000,
            )));
            let mut data = wincode::serialize(&state).unwrap();
            data.resize(NonceState::size() + 10, 0);
            Some(Account {
                lamports: 1,
                data,
                owner: solana_sdk_ids::system_program::id(),
                ..Account::default()
            })
        };
        let (before, after) = (
            Address::new_from_array([2; 32]),
            Address::new_from_array([3; 32]),
        );
        let (_, changes) = state_changes(
            &[address],
            &[nonce_account(before)],
            &[nonce_account(after)],
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let changes = changes.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            changes,
            [format!(
                "{address}  durable nonce authority: {before} -> {after}"
            )]
        );
    }
}
