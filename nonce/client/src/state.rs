//! Off-chain SPL Nonce account-state decoding.

use spl_nonce_interface::state::Nonce;

/// Decodes a complete initialized SPL Nonce account data buffer.
/// Returns `None` for uninitialized, malformed, or trailing data.
pub fn decode(account_data: &[u8]) -> Option<Nonce> {
    if account_data.iter().all(|byte| *byte == 0) {
        return None;
    }
    wincode::deserialize_exact(account_data).ok()
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
        assert!(decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn rejects_uninitialized_nonce_data() {
        assert!(decode(&[0; Nonce::LEN]).is_none());
    }

    #[test]
    fn rejects_trailing_nonce_data() {
        let mut account_data = [1; Nonce::LEN + 1];
        account_data[32..Nonce::LEN].fill(2);

        assert!(decode(&account_data).is_none());
    }
}
