use {
    solana_address::{ADDRESS_BYTES, Address},
    solana_hash::{HASH_BYTES, Hash},
    wincode::{SchemaRead, SchemaWrite},
};

/// Tag for nonce derivation.
pub const NONCE_DERIVATION_TAG: &[u8] = b"spl-nonce::v1";

/// On-chain data for a caller-created nonce account.
///
/// One authority can control any number of independent nonce accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently.
/// Each account carries its own nonce, so consuming one nonce does not advance or
/// invalidate messages prepared against another nonce account.
///
/// Consumers (e.g. message executor programs) read this state directly to verify a
/// wrapped message's lifetime field, then CPI `Advance` to consume the nonce.
///
/// The authority may be any address that can carry signer privilege, a keypair or any
/// program's PDA. This program is independent of the SPL Ed25519 Signer.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct Nonce {
    /// Single-use value that prevents a signed message from being replayed. `Advance`
    /// requires the caller to present this value and the stored authority's signer
    /// privilege, then replaces it with a fresh hash over the prior nonce and
    /// `SlotHashes[0]`.
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value. `Advance` requires
    /// this address's account to carry runtime signer privilege.
    pub authority: Address,
}

impl Nonce {
    /// Serialized account size is a 32-byte nonce followed by a 32-byte authority address.
    pub const LEN: usize = HASH_BYTES + ADDRESS_BYTES;
}

#[cfg(test)]
mod tests {
    use super::{Address, Hash, Nonce};

    #[test]
    fn len_matches_wincode_serialized_size() {
        let account = Nonce {
            nonce: Hash::new_from_array([1; 32]),
            authority: Address::new_from_array([2; 32]),
        };

        assert_eq!(
            wincode::serialized_size(&account).unwrap() as usize,
            Nonce::LEN
        );
    }
}
