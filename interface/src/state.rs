use {
    solana_address::Address,
    solana_hash::Hash,
    wincode::{SchemaRead, SchemaWrite},
};

/// On-chain state for a nonce account.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct NonceState {
    /// Single-use value that prevents a signed message from being replayed. `Submit` requires
    /// the wrapped v1 transaction message's lifetime specifier to equal this. On success,
    /// `Submit` advances it to a fresh hash over the prior nonce, `SlotHashes[0]`, and the
    /// wrapped message bytes.
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value. `Submit` verifies that this
    /// address signed the wrapped transaction message.
    pub authority: Address,
}
