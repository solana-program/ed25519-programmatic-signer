//! Verifier selection — **the one knob a fork flips before deploying.**
//!
//! The program is generic over a single [`Verifier`](crate::verifier::Verifier);
//! everything else is hardcoded. Point [`ActiveVerifier`] at a different impl and
//! redeploy to change the signature scheme — e.g. swap the standard ed25519
//! scheme for the post-quantum [`FalconVerifier`](crate::falcon::FalconVerifier):
//!
//! ```ignore
//! pub type ActiveVerifier = crate::falcon::FalconVerifier;
//! ```

use crate::verifier::Ed25519Verifier;

/// Signature scheme the deployed program enforces.
pub type ActiveVerifier = Ed25519Verifier;
