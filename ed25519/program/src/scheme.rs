use {
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_message::v1,
    spl_programmatic_signer_core::scheme::{AuthorizedSigners, SignerScheme},
};

pub struct UnimplementedScheme;

impl SignerScheme for UnimplementedScheme {
    type Authorization = ();

    fn authorize(
        _program_id: &Address,
        _accounts: &[AccountView],
        _message: &v1::Message,
        _message_bytes: &[u8],
        _authorizations: &[Self::Authorization],
    ) -> Result<AuthorizedSigners, ProgramError> {
        unimplemented!()
    }
}
