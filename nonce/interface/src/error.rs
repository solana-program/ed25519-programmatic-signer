#[cfg(feature = "codama")]
use codama_macros::CodamaErrors;
use {core::fmt, solana_program_error::ProgramError};

/// Errors that may be returned by the SPL Nonce program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
#[cfg_attr(feature = "codama", derive(CodamaErrors))]
pub enum Error {
    /// The nonce account is malformed or not writable.
    #[cfg_attr(
        feature = "codama",
        codama(error(message = "The nonce account is malformed or not writable"))
    )]
    InvalidNonceAccount = 0,
    /// The stored authority does not match the passed authority account.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "The stored authority does not match the passed authority account"
        ))
    )]
    AuthorityMismatch = 1,
    /// The stored nonce does not match the nonce supplied by the caller.
    #[cfg_attr(
        feature = "codama",
        codama(error(
            message = "The stored nonce does not match the nonce supplied by the caller"
        ))
    )]
    NonceMismatch = 2,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}

/// Errors returned when decoding SPL Nonce account data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The account data is correctly sized but contains the all-zero, uninitialized state.
    Uninitialized,
    /// The account data is malformed or contains trailing bytes.
    InvalidData,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => formatter.write_str("uninitialized SPL Nonce account"),
            Self::InvalidData => formatter.write_str("invalid SPL Nonce account data"),
        }
    }
}

impl core::error::Error for DecodeError {}
