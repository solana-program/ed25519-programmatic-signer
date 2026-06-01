//! Error types for the SPL Message Executor program.

use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Message Executor program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    /// The nonce account is malformed or not writable.
    InvalidNonceAccount = 0,
    /// The wrapped message failed Solana sanitization or violates an execute replay policy.
    InvalidMessage = 1,
    /// The passed accounts do not match the wrapped message's account keys or writability.
    MessageAccountsMismatch = 2,
    /// A wrapped message required signer was passed without signer privilege.
    MissingRequiredSigner = 3,
    /// The stored authority is not one of the wrapped message's leading required signer keys.
    AuthorityMismatch = 4,
    /// The wrapped message's `recent_blockhash` field does not match the stored nonce.
    NonceMismatch = 5,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
