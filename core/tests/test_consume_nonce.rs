use {
    crate::helpers::{
        common::{MOCK_PROGRAM_ID, init_mollusk},
        nonce_account_builder::NonceAccountBuilder,
    },
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    spl_programmatic_signer_core::{error::Error, instruction::consume_nonce},
};

pub mod helpers;

#[test]
fn consume_nonce_is_not_callable_directly() {
    let mollusk = init_mollusk();
    let authority = Address::new_unique();
    let (nonce_address, nonce_account) = NonceAccountBuilder::new().build();
    let ix = consume_nonce(&MOCK_PROGRAM_ID, &nonce_address, &authority);

    mollusk.process_and_validate_instruction(
        &ix,
        &[
            (nonce_address, nonce_account),
            (authority, Account::default()),
        ],
        &[Check::err(Error::ConsumeNonceNotCallable.into())],
    );
}
