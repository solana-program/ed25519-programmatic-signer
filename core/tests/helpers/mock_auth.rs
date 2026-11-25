use {
    solana_address::Address,
    wincode::{SchemaRead, SchemaWrite},
};

/// Tells the mock program how to treat each required signer during tests,
/// in place of real signatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum MockAuth {
    /// Leave this signer unauthorized.
    None,
    /// Treat this signer as a normal outer transaction signer (also set `is_signer = true` on `Execute`).
    TransactionSigner,
    /// Sign for this account as the `ProgrammaticSigner` PDA for the given authority.
    Programmatic(Address),
    /// Reject signing with `ProgramError::InvalidArgument`.
    Invalid,
}
