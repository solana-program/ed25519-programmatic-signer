//! Error types for the SPL Ed25519 Signer program.

use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Ed25519 Signer program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    /// The wrapped transaction failed sanitization.
    InvalidWrappedTransaction = 0,
    /// The wrapped transaction must contain exactly one executor instruction.
    InvalidExecutorInstructionCount = 1,
    /// A Submit account does not match the wrapped message account key at the same index.
    AccountKeyMismatch = 2,
    /// An authority signature failed verification against the wrapped message.
    InvalidSignature = 3,
    /// The executor instruction references an account index that is not a static wrapped account key.
    InvalidExecutorAccountIndex = 4,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
