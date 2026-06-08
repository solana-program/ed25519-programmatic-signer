//! Experimental post-quantum (Falcon-512) verifier.
//!
//! This is a **proof that the [`Verifier`] seam accommodates a quantum-resistant
//! scheme with no changes to the rest of the program** — not a production,
//! audited post-quantum implementation. It is not wired in by default; select it
//! in [`crate::config`] to deploy a Falcon variant.
//!
//! ## Why this needs no instruction-format change
//!
//! A Falcon-512 public key (~897 B) and signature (~666 B) dwarf ed25519's
//! 32/64 B. The wrapped transaction's signature slots are fixed at 64 bytes,
//! and SIMD-0385 ("V1 transactions") raised the *total*
//! transaction budget to 4 KB but did **not** widen those slots. So the Falcon
//! credential cannot ride `transaction.signatures`; instead the caller supplies
//! it in the **authority account's data** as `public_key ‖ signature`, which the
//! verifier reads through `&AccountView`. The ~1.5 KB credential fits well inside
//! SIMD-0385's 4 KB transaction budget.
//!
//! The durable signer's stored `authority` is that account's address, controlled
//! by its owner — so the account vouches for its declared Falcon key the same way
//! an ed25519 authority address *is* its key.
//!
//! ## What is still a stub
//!
//! [`falcon512_verify`] is a fail-closed placeholder: real Falcon verification is
//! compute-heavy and would need a `no_std` verifier (e.g. FN-DSA) or a runtime
//! precompile. It returns an error so this scheme can never silently accept a
//! signature. The surrounding plumbing — credential transport and seam wiring —
//! is real.

use {
    crate::verifier::Verifier,
    pinocchio::{AccountView, ProgramResult, error::ProgramError},
    spl_ed25519_durable_signer_interface::error::DurableSignerError,
};

/// Falcon-512 public key length, in bytes.
const FALCON512_PUBKEY_LEN: usize = 897;

/// Post-quantum verifier. The credential rides the authority account's data;
/// the 64-byte transaction signature slot is unused.
pub struct FalconVerifier;

impl Verifier for FalconVerifier {
    fn verify(authority: &AccountView, _signature: &[u8], message_bytes: &[u8]) -> ProgramResult {
        let credential = authority.try_borrow()?;
        if credential.len() <= FALCON512_PUBKEY_LEN {
            return Err(DurableSignerError::MissingAuthorization.into());
        }
        let (public_key, signature) = credential.split_at(FALCON512_PUBKEY_LEN);

        falcon512_verify(public_key, signature, message_bytes)
    }
}

/// Verifies a Falcon-512 `signature` over `message` under `public_key`.
///
/// TODO(post-quantum): wire real Falcon-512 verification here. On-chain
/// verification is compute-heavy, so a production deployment likely needs a
/// runtime precompile rather than in-BPF verification. Until then this fails
/// closed so the scheme can never accept an unverified signature.
fn falcon512_verify(
    _public_key: &[u8],
    _signature: &[u8],
    _message: &[u8],
) -> Result<(), ProgramError> {
    Err(DurableSignerError::MissingAuthorization.into())
}
