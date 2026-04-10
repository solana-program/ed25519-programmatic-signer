//! PDA derivation helpers for the SPL Nonce program.
//!
//! One [`AuthorityPolicy`] determines two canonical PDAs:
//! [`NonceStatePda`] and [`NonceAuthorityPda`].

use {crate::state::AuthorityPolicy, solana_address::Address};

/// Nonce state account PDA. Stores the nonce counter and authority policy.
///
/// Seeds: `["nonce-state", authority_policy_hash, bump]`
pub struct NonceStatePda;

impl NonceStatePda {
    pub const SEED_PREFIX: &[u8] = b"nonce-state";

    #[inline(always)]
    pub fn derive_address_and_bump(
        program_id: &Address,
        authority_policy: &AuthorityPolicy,
    ) -> (Address, u8) {
        let authority_policy_hash = authority_policy.hash();
        Address::derive_program_address(&[Self::SEED_PREFIX, &authority_policy_hash], program_id)
            .expect("failed to derive NonceStatePda from authority policy")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, authority_policy: &AuthorityPolicy) -> Address {
        Self::derive_address_and_bump(program_id, authority_policy).0
    }
}

/// Nonce authority PDA.
///
/// The PDA the program signs as when executing committed CPI instructions.
/// Downstream programs can recognize this address as an owner or authority.
///
/// Seeds: `["nonce-authority", authority_policy_hash, bump]`
pub struct NonceAuthorityPda;

impl NonceAuthorityPda {
    pub const SEED_PREFIX: &[u8] = b"nonce-authority";

    #[inline(always)]
    pub fn derive_address_and_bump(
        program_id: &Address,
        authority_policy: &AuthorityPolicy,
    ) -> (Address, u8) {
        let authority_policy_hash = authority_policy.hash();
        Address::derive_program_address(&[Self::SEED_PREFIX, &authority_policy_hash], program_id)
            .expect("failed to derive NonceAuthorityPda from authority policy")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, authority_policy: &AuthorityPolicy) -> Address {
        Self::derive_address_and_bump(program_id, authority_policy).0
    }
}
