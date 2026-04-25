//! PDA derivation helpers for the SPL Nonce program.
//!
//! A nonce account has two stable identities:
//! - [`NonceStatePda`], derived from the authority address
//! - [`NonceAuthorityPda`], derived from the nonce state address

use solana_address::Address;

/// Nonce state account PDA. Stores the replay-protection nonce and authority.
///
/// Seeds: `["nonce-state", authority, bump]`
pub struct NonceStatePda;

impl NonceStatePda {
    pub const SEED_PREFIX: &[u8] = b"nonce-state";

    #[inline(always)]
    pub fn derive_address_and_bump(program_id: &Address, authority: &Address) -> (Address, u8) {
        Address::derive_program_address(&[Self::SEED_PREFIX, authority.as_ref()], program_id)
            .expect("failed to derive NonceStatePda from authority")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, authority: &Address) -> Address {
        let (address, _bump) = Self::derive_address_and_bump(program_id, authority);
        address
    }
}

/// Nonce authority PDA.
///
/// Program-owned runtime signer for the nonce account. `Submit` promotes it to `is_signer=true`
/// via `invoke_signed` wherever the wrapped message references it.
///
/// Seeds: `["nonce-authority", nonce_state_addr, bump]`
pub struct NonceAuthorityPda;

impl NonceAuthorityPda {
    pub const SEED_PREFIX: &[u8] = b"nonce-authority";

    #[inline(always)]
    pub fn derive_address_and_bump(
        program_id: &Address,
        nonce_state_addr: &Address,
    ) -> (Address, u8) {
        Address::derive_program_address(&[Self::SEED_PREFIX, nonce_state_addr.as_ref()], program_id)
            .expect("failed to derive NonceAuthorityPda from nonce state address")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, nonce_state_address: &Address) -> Address {
        let (address, _bump) = Self::derive_address_and_bump(program_id, nonce_state_address);
        address
    }
}
