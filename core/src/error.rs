use core::fmt;

/// Errors returned when decoding nonce account data.
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
            Self::Uninitialized => formatter.write_str(
                "The account data is correctly sized but contains the all-zero, uninitialized \
                 state.",
            ),
            Self::InvalidData => {
                formatter.write_str("The account data is malformed or contains trailing bytes.")
            }
        }
    }
}

impl core::error::Error for DecodeError {}
