//! Account and authority changes between the pre- and post-simulation state of the accounts a
//! transaction writes to.

use {
    serde::Serialize,
    solana_account::Account,
    solana_address::Address,
    solana_clock::Epoch,
    solana_stake_interface::state::StakeStateV2,
    solana_vote_interface::state::VoteStateVersions,
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
    /// First epoch the authority applies to, for authorities that change at an epoch boundary.
    /// Omitted for other authorities.
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_epoch: Option<Epoch>,
    /// Signing pubkey whose programmatic signer is this address, if known. Omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_pubkey: Option<String>,
}

/// The kind of a parsed account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum AccountKind {
    Stake,
    Vote,
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
}

/// An authority, as parsed from account data.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ParsedAuthority {
    address: Address,
    effective_epoch: Option<Epoch>,
}

impl ParsedAuthority {
    fn new(address: Address) -> Self {
        Self {
            address,
            effective_epoch: None,
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
/// pubkeys, to label authority changes.
pub(super) fn state_changes<'a>(
    addresses: &[Address],
    pre: &'a [Option<Account>],
    post: &'a [Option<Account>],
    pda_signing_pubkeys: &BTreeMap<String, String>,
) -> (Vec<AccountChange>, Vec<AuthorityChange>) {
    let mut account_changes = Vec::new();
    let mut authority_changes = Vec::new();
    for ((address, pre), post) in addresses.iter().zip(pre).zip(post) {
        let exists =
            |account: &'a Option<Account>| account.as_ref().filter(|account| account.lamports > 0);
        let (pre, post) = (exists(pre), exists(post));
        account_changes.extend(account_change(address, pre, post));
        authority_changes.extend(authority_changes_for(
            address,
            pre.and_then(parse_account),
            post.and_then(parse_account),
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
    let authorities = |parsed: &Option<ParsedAccount>| {
        parsed
            .as_ref()
            .map(|parsed| parsed.authorities.clone())
            .unwrap_or_default()
    };
    let (pre, post) = (authorities(&pre), authorities(&post));
    let value = |authority: Option<&ParsedAuthority>| {
        authority.map(|authority| {
            let address = authority.address.to_string();
            AuthorityValue {
                signing_pubkey: pda_signing_pubkeys.get(&address).cloned(),
                address,
                effective_epoch: authority.effective_epoch,
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
/// unknown or the data is invalid.
fn parse_account(account: &Account) -> Option<ParsedAccount> {
    if account.owner == solana_stake_interface::program::id() {
        parse_stake(account)
    } else if account.owner == solana_sdk_ids::vote::id() {
        parse_vote(account)
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
                effective_epoch: Some(*epoch),
            },
        );
    }
    Some(ParsedAccount {
        kind: AccountKind::Vote,
        authorities,
    })
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
        })
    }
}

impl fmt::Display for AuthorityValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address)?;
        if let Some(signing_pubkey) = &self.signing_pubkey {
            write!(f, " (PDA of {signing_pubkey})")?;
        }
        if let Some(epoch) = self.effective_epoch {
            write!(f, " (from epoch {epoch})")?;
        }
        Ok(())
    }
}
