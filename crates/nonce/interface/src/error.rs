//! Error types for the SPL Nonce program.

use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Nonce program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    /// The nonce account is malformed or not writable.
    InvalidNonceAccount = 0,
    /// The stored authority does not match the passed authority account.
    AuthorityMismatch = 1,
    /// The stored nonce does not match the nonce supplied by the caller.
    NonceMismatch = 2,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
