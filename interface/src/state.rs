use {
    solana_address::Address,
    solana_hash::Hash,
    wincode::{SchemaRead, SchemaWrite},
};

/// On-chain state for a nonce account. Caller-created and owned by the nonce program.
///
/// One authority can control any number of independent nonce state accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently. Each
/// account carries its own nonce, so consuming one nonce does not advance or invalidate
/// transactions prepared against another nonce state account.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct NonceState {
    /// Single-use value that prevents a signed message from being replayed. `Submit` requires this
    /// to match the wrapped message's lifetime field: `lifetime_specifier` for `v1` messages or
    /// `recent_blockhash` for `legacy` and `v0` messages. On success, `Submit` advances it to a
    /// fresh hash over the prior nonce, `SlotHashes[0]`, and the wrapped message bytes.
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value. `Submit` verifies that this
    /// address signed the wrapped transaction message.
    pub authority: Address,
}
