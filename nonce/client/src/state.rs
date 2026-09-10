//! Off-chain SPL Nonce account-state decoding.

use {crate::error::DecodeError, spl_nonce_interface::state::Nonce};

/// Decodes initialized SPL Nonce account data.
pub fn decode(account_data: &[u8]) -> Result<Nonce, DecodeError> {
    let nonce = wincode::deserialize_exact(account_data).map_err(|_| DecodeError::InvalidData)?;

    // A newly allocated nonce account is zero filled until initialized. Wincode will deserialize
    // this successfully with hash/address fields as 111... so we handle that case here.
    if account_data.iter().all(|byte| *byte == 0) {
        return Err(DecodeError::Uninitialized);
    }

    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use {super::*, solana_hash::Hash};

    #[test]
    fn decodes_initialized_nonce_data() {
        let mut account_data = [1; Nonce::LEN];
        account_data[32..].fill(2);

        assert_eq!(
            decode(&account_data).unwrap(),
            Nonce {
                nonce: Hash::new_from_array([1; 32]),
                authority: solana_address::Address::new_from_array([2; 32]),
            }
        );
    }

    #[test]
    fn rejects_malformed_nonce_data() {
        assert_eq!(decode(&[1, 2, 3]), Err(DecodeError::InvalidData));
    }

    #[test]
    fn rejects_uninitialized_nonce_data() {
        assert_eq!(decode(&[0; Nonce::LEN]), Err(DecodeError::Uninitialized));
    }

    #[test]
    fn rejects_wrong_sized_zeroed_data() {
        let account_data = [0; Nonce::LEN + 1];
        for len in [0, Nonce::LEN - 1, Nonce::LEN + 1] {
            assert_eq!(decode(&account_data[..len]), Err(DecodeError::InvalidData));
        }
    }

    #[test]
    fn rejects_trailing_nonce_data() {
        let mut account_data = [1; Nonce::LEN + 1];
        account_data[32..Nonce::LEN].fill(2);

        assert_eq!(decode(&account_data), Err(DecodeError::InvalidData));
    }
}
