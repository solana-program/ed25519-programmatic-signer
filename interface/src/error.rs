//! Error types for the SPL Ed25519 Durable Signer program.

use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Ed25519 Durable Signer program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DurableSignerError {
    /// The stored durable signer account bytes are malformed.
    InvalidDurableSignerAccount = 1,
    /// The wrapped transaction's lifetime field does not match the stored nonce.
    NonceMismatch = 2,
    /// The configured authority is not in the wrapped transaction's
    /// required-signer prefix.
    AuthorityMismatch = 3,
    /// A configured authority failed to authorize the wrapped message with a
    /// valid transaction signature.
    MissingAuthorization = 4,
    /// The wrapped transaction failed Solana sanitization or violates a
    /// submit replay policy.
    InvalidWrappedTransaction = 5,
    /// The outer submit accounts do not match the wrapped transaction's
    /// `message.account_keys`.
    WrappedMessageAccountsMismatch = 6,
    /// A wrapped instruction required a signer that is not one of the
    /// authorized `DurableSignerPda` accounts.
    MissingRequiredSigner = 7,
    /// The provided durable signer PDA account is incorrect.
    IncorrectAuthorityPda = 8,
    /// The `SlotHashes` sysvar did not contain a current entry.
    SlotHashesUnavailable = 11,
    /// The Instructions sysvar account passed to Submit is not the correct
    /// sysvar.
    InvalidInstructionsSysvar = 15,
    /// The outer transaction's only top-level instruction was not direct `Submit`.
    OuterTxMustContainOnlySubmit = 16,
}

impl From<DurableSignerError> for ProgramError {
    fn from(error: DurableSignerError) -> Self {
        ProgramError::Custom(error as u32)
    }
}
