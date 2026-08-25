//! Error types for the SPL Ed25519 Signer program.

#[cfg(feature = "codama")]
use codama_macros::CodamaErrors;
use solana_program_error::ProgramError;

/// Errors that may be returned by the SPL Ed25519 Signer program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
#[cfg_attr(feature = "codama", derive(CodamaErrors))]
pub enum Error {
    /// The wrapped transaction failed sanitization.
    #[cfg_attr(
        feature = "codama",
        codama(error(message = "The wrapped transaction failed sanitization"))
    )]
    InvalidWrappedTransaction = 0,
    /// The wrapped transaction must contain exactly one executor instruction.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "The wrapped transaction must contain exactly one executor instruction"
        ))
    )]
    InvalidExecutorInstructionCount = 1,
    /// A Submit account differs from the wrapped message key at the same index.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "A Submit account differs from the wrapped message key at the same index"
        ))
    )]
    AccountKeyMismatch = 2,
    /// An authority signature failed verification against the wrapped message.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "An authority signature failed verification against the wrapped message"
        ))
    )]
    InvalidSignature = 3,
    /// The executor references an index outside the static account-key list.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "The executor references an index outside the static account-key list"
        ))
    )]
    InvalidExecutorAccountIndex = 4,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
