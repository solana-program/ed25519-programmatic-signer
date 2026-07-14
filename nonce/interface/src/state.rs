use {
    solana_address::{ADDRESS_BYTES, Address},
    solana_hash::{HASH_BYTES, Hash},
    solana_sha256_hasher::hashv,
    wincode::{SchemaRead, SchemaWrite},
};

pub const NONCE_INIT_TAG: &[u8] = b"spl-nonce::init::v1";
pub const NONCE_STEP_TAG: &[u8] = b"spl-nonce::step::v1";

/// On-chain data for a caller-created nonce account.
///
/// One authority can control any number of independent nonce accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct Nonce {
    /// Single-use value that prevents a signed message from being replayed. `Advance`
    /// requires the caller to present this value and the stored authority's signer
    /// privilege, then replaces it with a freshly derived hash.
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value.
    pub authority: Address,
}

impl Nonce {
    pub const LEN: usize = HASH_BYTES + ADDRESS_BYTES;

    /// Derives the value for a newly initialized nonce account.
    pub fn derive_initial_value(
        program_id: &Address,
        nonce_account: &Address,
        recent_slot_hash: &Hash,
    ) -> Hash {
        hashv(&[
            NONCE_INIT_TAG,               // domain-separates initialization from advancement
            &program_id.to_bytes(),       // binds the derivation to the program address
            &nonce_account.to_bytes(),    // binds the initial value to the nonce-account address
            &recent_slot_hash.to_bytes(), // makes reinitialization differ when the latest slot hash changes
        ])
    }

    /// Derives the value that follows this nonce for the given recent slot hash.
    pub fn derive_next_value(
        &self,
        program_id: &Address,
        nonce_account: &Address,
        recent_slot_hash: &Hash,
    ) -> Hash {
        hashv(&[
            NONCE_STEP_TAG,               // domain-separates advancement from initialization
            &program_id.to_bytes(),       // binds the derivation to the program address
            &nonce_account.to_bytes(),    // binds each successor to the nonce-account address
            &self.nonce.to_bytes(),       // makes the successor depend on the current nonce
            &recent_slot_hash.to_bytes(), // makes the successor depend on the recent slot hash
        ])
    }
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

    #[test]
    fn derivations_match_frozen_vectors() {
        let program_id = Address::from([1; 32]);
        let nonce_account = Address::from([2; 32]);
        let recent_slot_hash = Hash::from([3; 32]);

        assert_eq!(
            Nonce::derive_initial_value(&program_id, &nonce_account, &recent_slot_hash),
            "vDVVCR9vGGZ7RKg1RHT3Bgtn8VgaBEexvVfvniSZ4xc"
                .parse::<Hash>()
                .unwrap()
        );

        let state = Nonce {
            nonce: "GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq"
                .parse::<Hash>()
                .unwrap(),
            authority: Address::default(),
        };
        assert_eq!(
            state.derive_next_value(&program_id, &nonce_account, &recent_slot_hash),
            "8Sh5B8dWH6xhSmHdwxLPWVifNFRXdGrd8nA9G7RMcVg4"
                .parse::<Hash>()
                .unwrap()
        );
    }
}
