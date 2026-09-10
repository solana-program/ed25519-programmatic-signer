use core::fmt;

/// Errors returned when decoding SPL Nonce account data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The account data is correctly sized but has not been initialized.
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
