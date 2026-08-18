#[cfg(feature = "codama")]
use codama_macros::CodamaAccount;
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
#[wincode(assert_zero_copy)]
#[repr(C)]
#[cfg_attr(
    feature = "codama",
    derive(CodamaAccount),
    codama(discriminator(size = 64))
)]
pub struct Nonce {
    /// Single-use value that prevents a signed message from being replayed. `Advance`
    /// requires the caller to present this value and the stored authority's signer
    /// privilege, then replaces it with a freshly derived hash.
    #[cfg_attr(feature = "codama", codama(type = public_key))]
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value.
    pub authority: Address,
}

impl Nonce {
    pub const LEN: usize = HASH_BYTES + ADDRESS_BYTES;

    /// Derives the value for a newly initialized nonce account.
    pub fn derive_initial_nonce(
        program_id: &Address,
        nonce_account: &Address,
        recent_slot_hash: &Hash,
    ) -> Hash {
        hashv(&[
            NONCE_INIT_TAG,              // domain-separates initialization from advancement
            program_id.as_array(),       // binds the derivation to the program address
            nonce_account.as_array(),    // binds the initial value to the nonce-account address
            recent_slot_hash.as_bytes(), // makes reinitialization differ when the latest slot hash changes
        ])
    }

    /// Derives the value that follows this nonce.
    pub fn derive_next_nonce(
        &self,
        program_id: &Address,
        nonce_account: &Address,
        transition_commitment: &Hash,
    ) -> Hash {
        hashv(&[
            NONCE_STEP_TAG,                   // domain-separates advancement from initialization
            program_id.as_array(),            // binds the derivation to the program address
            nonce_account.as_array(),         // binds each successor to the nonce-account address
            self.nonce.as_bytes(),            // makes the successor depend on the current nonce
            transition_commitment.as_bytes(), // binds the successor to the action this step authorizes
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
        let transition_commitment = Hash::from([4; 32]);

        assert_eq!(
            Nonce::derive_initial_nonce(&program_id, &nonce_account, &recent_slot_hash),
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
            state.derive_next_nonce(&program_id, &nonce_account, &transition_commitment),
            "EgbzChWYoCDgPbJNWv8nVUqdxnoxDTRXg2stzPKhYZu4"
                .parse::<Hash>()
                .unwrap()
        );
    }
}
