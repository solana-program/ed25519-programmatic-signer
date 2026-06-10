//! Error types for the SPL Ed25519 Signer program.

use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Ed25519 Signer program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    /// The executor program account does not match the signed payload's `executor_program_id`.
    ExecutorMismatch,
    /// A provided signature failed to verify against its authority.
    InvalidSignature,
    /// The envelope carried no signatures, so it authorizes nothing.
    NoSignatures,
    /// The signed payload's `signer_program_id` is not this program's id.
    SignerProgramMismatch,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
