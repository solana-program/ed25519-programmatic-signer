use solana_program_error::ProgramError;

/// Custom errors returned by the SPL Message Executor program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    /// The nonce account data could not be decoded as nonce state.
    InvalidNonceAccount = 0,
    /// The message has an unsupported shape, fails sanitization, or contains duplicate keys.
    InvalidMessage = 1,
    /// The passed accounts do not match the wrapped message's account keys or writability.
    MessageAccountsMismatch = 2,
    /// A wrapped message required signer was passed without signer privilege.
    MissingRequiredSigner = 3,
    /// The nonce account's stored authority is not a required message signer.
    MissingNonceAuthoritySigner = 4,
    /// The message's lifetime specifier does not match the stored nonce.
    NonceMismatch = 5,
}

impl From<Error> for ProgramError {
    fn from(error: Error) -> Self {
        ProgramError::Custom(error as u32)
    }
}
