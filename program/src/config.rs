//! Signing scheme selection.
//!
//! The program is generic over a single [`SigningScheme`](crate::verifier::SigningScheme);
//! everything else is hard-coded. The active scheme is chosen at build time:
//! standard Ed25519 by default, or the post-quantum
//! [`FalconScheme`](crate::falcon::FalconScheme) with `--features falcon`.

/// Signature scheme the deployed program enforces (standard Solana Ed25519).
#[cfg(not(feature = "falcon"))]
pub type ActiveScheme = crate::verifier::Ed25519Scheme;

/// Signature scheme the deployed program enforces (post-quantum Falcon-512).
#[cfg(feature = "falcon")]
pub type ActiveScheme = crate::falcon::FalconScheme;
