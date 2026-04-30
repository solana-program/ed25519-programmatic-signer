use solana_address::Address;

/// Nonce authority PDA.
///
/// Program-owned runtime signer for an authority. `Submit` promotes it to `is_signer=true`
/// via `invoke_signed` wherever a wrapped instruction references it after the corresponding
/// authority has signed the wrapped message.
///
/// Seeds: `["nonce-authority", authority, bump]`
pub struct NonceAuthorityPda;

impl NonceAuthorityPda {
    pub const SEED_PREFIX: &[u8] = b"nonce-authority";

    #[inline(always)]
    pub fn derive_address_and_bump(program_id: &Address, authority: &Address) -> (Address, u8) {
        Address::derive_program_address(&[Self::SEED_PREFIX, authority.as_ref()], program_id)
            .expect("failed to derive NonceAuthorityPda from authority")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, authority: &Address) -> Address {
        let (address, _bump) = Self::derive_address_and_bump(program_id, authority);
        address
    }
}
