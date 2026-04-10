use {
    alloc::vec::Vec,
    solana_address::Address,
    solana_sha256_hasher::hashv,
    solana_zero_copy::unaligned::U32,
    wincode::{SchemaRead, SchemaWrite, containers},
};

/// On-chain state for a nonce account.
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
pub struct NonceState {
    /// Counter that prevents reuse of signed messages. A signed message must
    /// reference this exact value. Each successful signed action increments
    /// this value, invalidating any previously signed messages.
    pub nonce: U32,
    /// The set of keys authorized to sign actions for this account.
    pub authority_policy: AuthorityPolicy,
}

/// Signer policy that describes who is allowed to authorize use of a nonce account.
/// Supports both single-signer and threshold-multisig flows.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct AuthorityPolicy {
    /// Number of member approvals required to authorize execution.
    pub threshold: u8,
    /// Authority members in canonical ascending address-byte order.
    /// The stored order is semantically meaningful:
    /// - signature entries identify members by index in this list
    /// - the policy hash commits to members in this order
    #[wincode(with = "containers::Vec<Address, u8>")]
    pub members: Vec<Address>,
}

impl AuthorityPolicy {
    /// Returns the SHA-256 digest of this policy, used as a seed in PDA
    /// derivation for the nonce state account. The member list is hashed in
    /// the stored order.
    pub fn hash(&self) -> [u8; 32] {
        let threshold = [self.threshold];
        let member_count = [self.members.len() as u8];
        let capacity = self.members.len().saturating_add(2);
        let mut segments: Vec<&[u8]> = Vec::with_capacity(capacity);
        segments.push(&threshold);
        segments.push(&member_count);
        segments.extend(self.members.iter().map(Address::as_ref));
        hashv(&segments).to_bytes()
    }
}
