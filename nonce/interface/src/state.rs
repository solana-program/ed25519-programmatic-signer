#[cfg(feature = "codama")]
use codama_macros::CodamaAccount;
use {
    crate::error::DecodeError,
    solana_address::{ADDRESS_BYTES, Address},
    solana_hash::{HASH_BYTES, Hash},
    solana_sha256_hasher::hashv,
    wincode::{SchemaRead, SchemaWrite, ZeroCopy},
};

pub const NONCE_INIT_TAG: &[u8] = b"spl-nonce::init::v1";
pub const NONCE_STEP_TAG: &[u8] = b"spl-nonce::step::v1";

/// On-chain data for a caller-created nonce account.
///
/// One authority can control any number of independent nonce accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently.
#[derive(Clone, Debug, Default, PartialEq, Eq, SchemaRead, SchemaWrite)]
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

    /// Borrows nonce state from account data.
    ///
    /// Requires exactly [`Self::LEN`] bytes and accepts the all-zero, uninitialized state.
    /// Use [`Self::view_initialized`] when initialized state is required.
    #[inline]
    pub fn view(account_data: &[u8]) -> Result<&Self, DecodeError> {
        if account_data.len() != Self::LEN {
            return Err(DecodeError::InvalidData);
        }
        Self::from_bytes(account_data).map_err(|_| DecodeError::InvalidData)
    }

    /// Mutably borrows nonce state from account data.
    ///
    /// Performs the same checks as [`Self::view`], accepting uninitialized state.
    /// Changes to the returned state update the account data in place.
    #[inline]
    pub fn view_mut(account_data: &mut [u8]) -> Result<&mut Self, DecodeError> {
        if account_data.len() != Self::LEN {
            return Err(DecodeError::InvalidData);
        }
        Self::from_bytes_mut(account_data).map_err(|_| DecodeError::InvalidData)
    }

    /// Borrows initialized nonce state from account data.
    ///
    /// Requires exactly [`Self::LEN`] bytes and rejects the all-zero, uninitialized state.
    #[inline]
    pub fn view_initialized(account_data: &[u8]) -> Result<&Self, DecodeError> {
        let state = Self::view(account_data)?;
        if !state.is_initialized() {
            return Err(DecodeError::Uninitialized);
        }
        Ok(state)
    }

    /// Mutably borrows initialized nonce state from account data.
    ///
    /// Performs the same checks as [`Self::view_initialized`]. Changes to the returned state
    /// update the account data in place.
    #[inline]
    pub fn view_initialized_mut(account_data: &mut [u8]) -> Result<&mut Self, DecodeError> {
        let state = Self::view_mut(account_data)?;
        if !state.is_initialized() {
            return Err(DecodeError::Uninitialized);
        }
        Ok(state)
    }

    /// Returns whether the nonce state is initialized, meaning either field is nonzero.
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self != &Self::default()
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use {
        super::{Address, Hash, Nonce},
        crate::error::DecodeError,
        alloc::vec,
        test_case::test_case,
    };

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

    #[test_case(1, 0; "zero authority")]
    #[test_case(0, 2; "zero nonce")]
    #[test_case(1, 2; "both fields nonzero")]
    fn views_decode_initialized_state(nonce: u8, authority: u8) {
        let mut data = [nonce; Nonce::LEN];
        data[32..].fill(authority);
        let expected = Nonce {
            nonce: Hash::new_from_array([nonce; 32]),
            authority: Address::new_from_array([authority; 32]),
        };

        assert!(expected.is_initialized());
        assert_eq!(Nonce::view(&data).unwrap(), &expected);
        assert_eq!(Nonce::view_mut(&mut data).unwrap(), &expected);
        assert_eq!(Nonce::view_initialized(&data).unwrap(), &expected);
        assert_eq!(Nonce::view_initialized_mut(&mut data).unwrap(), &expected);
    }

    #[test]
    fn views_handle_uninitialized_state() {
        let mut data = [0; Nonce::LEN];
        let expected = Nonce::default();

        assert!(!expected.is_initialized());
        assert_eq!(Nonce::view(&data).unwrap(), &expected);
        assert_eq!(Nonce::view_mut(&mut data).unwrap(), &expected);
        assert_eq!(
            Nonce::view_initialized(&data),
            Err(DecodeError::Uninitialized)
        );
        assert_eq!(
            Nonce::view_initialized_mut(&mut data),
            Err(DecodeError::Uninitialized)
        );
    }

    #[test_case(0, 0; "empty")]
    #[test_case(63, 0; "truncated zeroed")]
    #[test_case(63, 1; "truncated nonzero")]
    #[test_case(65, 0; "trailing zeroed")]
    #[test_case(65, 1; "trailing nonzero")]
    fn views_reject_wrong_lengths(len: usize, value: u8) {
        let mut data = vec![value; len];
        assert_eq!(Nonce::view(&data), Err(DecodeError::InvalidData));
        assert_eq!(Nonce::view_mut(&mut data), Err(DecodeError::InvalidData));
        assert_eq!(
            Nonce::view_initialized(&data),
            Err(DecodeError::InvalidData)
        );
        assert_eq!(
            Nonce::view_initialized_mut(&mut data),
            Err(DecodeError::InvalidData)
        );
    }

    #[test_case(Nonce::view_mut, 0; "initialize")]
    #[test_case(Nonce::view_initialized_mut, 1; "update")]
    fn mutable_views_update_the_original_bytes(
        view: fn(&mut [u8]) -> Result<&mut Nonce, DecodeError>,
        initial: u8,
    ) {
        let mut data = [initial; Nonce::LEN];
        let state = view(&mut data).unwrap();
        *state = Nonce {
            nonce: Hash::new_from_array([2; 32]),
            authority: Address::new_from_array([3; 32]),
        };

        assert_eq!(&data[..32], &[2; 32]);
        assert_eq!(&data[32..], &[3; 32]);
    }
}
