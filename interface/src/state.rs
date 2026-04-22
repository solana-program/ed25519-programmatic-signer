use {
    solana_address::Address,
    solana_hash::Hash,
    wincode::{SchemaRead, SchemaWrite},
};

/// On-chain state for a nonce account.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct NonceState {
    /// Single-use value that prevents a signed message from being replayed. `Submit` requires
    /// the wrapped `Transaction`'s `message.recent_blockhash` to equal this. On success,
    /// `Submit` advances it to a fresh hash over the prior nonce, `SlotHashes[0]`, and the
    /// wrapped message bytes.
    pub nonce: Hash,
    /// First required signer of every `Submit`. Pinned at `tx.message.account_keys[0]`.
    /// Any further signer positions in the wrapped message's signer prefix are verified
    /// the same way (see `NonceInstruction::Submit`) but have no special status in state.
    pub authority: Address,
}
