//! The program's one pluggable seam: how a wrapped message is proven authorized.
//!
//! Everything else in the processor is hardcoded. Only the signature scheme is
//! swappable, selected in [`crate::config`]. This is enough to run the program
//! under a standard Solana scheme ([`Ed25519Verifier`]) or a post-quantum one
//! ([`crate::falcon::FalconVerifier`]) by editing one line and redeploying.

use {
    brine_ed25519::{hasher::Sha512, verify},
    pinocchio::{AccountView, ProgramResult, error::ProgramError},
    spl_ed25519_durable_signer_interface::error::DurableSignerError,
};

/// Proves that an authority approved a wrapped message.
///
/// The whole authority account is passed so a scheme can use whatever identity
/// material it needs: ed25519 reads the 32-byte `address()`, while schemes with
/// larger keys/signatures (e.g. Falcon) read them from the account's data. The
/// account's address is always the `DurableSignerPda` seed, so a `verify` impl
/// must bind whatever key it uses back to that address.
pub trait Verifier {
    /// Returns `Ok(())` only if the authority authorized `message_bytes`.
    ///
    /// `signature` is the wrapped transaction's 64-byte signature slot; schemes
    /// that carry their signature elsewhere (e.g. in account data) may ignore it.
    fn verify(authority: &AccountView, signature: &[u8], message_bytes: &[u8]) -> ProgramResult;
}

/// Standard Solana verification: the authority address *is* the ed25519 public
/// key, and the signature rides the wrapped transaction's signature slot.
pub struct Ed25519Verifier;

impl Verifier for Ed25519Verifier {
    fn verify(authority: &AccountView, signature: &[u8], message_bytes: &[u8]) -> ProgramResult {
        let pubkey: &[u8; 32] = authority
            .address()
            .as_ref()
            .try_into()
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;
        let signature: &[u8; 64] = signature
            .try_into()
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;

        verify::<Sha512>(pubkey, signature, &[message_bytes])
            .map_err(|_| DurableSignerError::MissingAuthorization.into())
    }
}
