#[cfg(feature = "codama")]
use codama_macros::CodamaPda;
use solana_address::Address;

/// Executor PDA that prevents the nonce authority from skipping steps in a signed chain. The nonce
/// program will bind the next nonce hash to this program via the signature of this PDA.
/// That means only this program will be able to advance message sequences signed in advance
/// that expect execution via this executor program.
#[cfg_attr(
    feature = "codama",
    derive(CodamaPda),
    codama(seed(type = string(utf8), value = "transition-signer"))
)]
#[derive(Clone, Copy, Debug)]
pub struct TransitionSigner;

impl TransitionSigner {
    pub const SEED_PREFIX: &[u8] = b"transition-signer";
    pub const BUMP: u8 = 255;
    pub const SIGNER_SEEDS: [&'static [u8]; 2] = [Self::SEED_PREFIX, &[Self::BUMP]];
    pub const ADDRESS: Address =
        Address::derive_address_const(&Self::SIGNER_SEEDS, None, &crate::ID);
}

#[cfg(test)]
mod tests {
    use {super::TransitionSigner, solana_address::Address};

    #[test]
    fn constants_match_derivation() {
        assert_eq!(
            Address::derive_program_address(&[TransitionSigner::SEED_PREFIX], &crate::ID),
            Some((TransitionSigner::ADDRESS, TransitionSigner::BUMP)),
        );
    }
}
