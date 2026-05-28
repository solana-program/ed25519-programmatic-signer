use solana_address::Address;

/// Durable signer PDA.
///
/// Program-owned runtime signer for an authority. `Submit` promotes it to `is_signer=true`
/// via `invoke_signed` wherever a wrapped instruction references it after the corresponding
/// authority has signed the wrapped message.
///
/// Seeds: `["durable-signer", authority, bump]`
pub struct DurableSignerPda;

impl DurableSignerPda {
    pub const SEED_PREFIX: &[u8] = b"durable-signer";

    #[inline(always)]
    pub fn derive_address_and_bump(program_id: &Address, authority: &Address) -> (Address, u8) {
        Address::derive_program_address(&[Self::SEED_PREFIX, authority.as_ref()], program_id)
            .expect("failed to derive DurableSignerPda from authority")
    }

    #[inline(always)]
    pub fn derive_address(program_id: &Address, authority: &Address) -> Address {
        let (address, _bump) = Self::derive_address_and_bump(program_id, authority);
        address
    }
}
