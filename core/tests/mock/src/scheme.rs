use {
    alloc::vec::Vec,
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_message::v1,
    spl_programmatic_signer_core::{
        pda::ProgrammaticSigner,
        scheme::{AuthorizedSigners, DerivedSigner, SignerScheme},
    },
    wincode::SchemaRead,
};

#[derive(SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum MockAuth {
    /// Do not authorize this signer slot for nonce consumption or CPI.
    None,
    /// Authorize this slot as an outer transaction signer (pair with `is_signer = true` on `Execute`).
    TransactionSigner,
    /// Authorize this slot as the [`ProgrammaticSigner`] PDA derived from the given authority.
    Programmatic(Address),
    /// Fail `SignerScheme::authorize` with [`ProgramError::InvalidArgument`].
    Invalid,
}

/// Test [`SignerScheme`] that skips signature verification and follows [`MockAuth`] directly.
pub struct MockScheme;

impl SignerScheme for MockScheme {
    type Authorization = MockAuth;

    fn authorize(
        program_id: &Address,
        _accounts: &[AccountView],
        _message: &v1::Message,
        _message_bytes: &[u8],
        authorizations: &[Self::Authorization],
    ) -> Result<AuthorizedSigners, ProgramError> {
        let mut programmatic_signers = Vec::new();
        let mut transaction_signers = Vec::new();

        for (index, auth) in authorizations.iter().enumerate() {
            match auth {
                MockAuth::None => {}
                MockAuth::TransactionSigner => transaction_signers.push(index),
                MockAuth::Programmatic(authority) => {
                    let (_, bump) =
                        ProgrammaticSigner::derive_address_and_bump(program_id, authority);
                    programmatic_signers.push(DerivedSigner {
                        account_index: index,
                        authority: *authority,
                        bump,
                    });
                }
                MockAuth::Invalid => return Err(ProgramError::InvalidArgument),
            }
        }

        Ok(AuthorizedSigners {
            programmatic_signers,
            transaction_signers,
        })
    }
}
