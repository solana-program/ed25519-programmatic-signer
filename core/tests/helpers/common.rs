use {
    crate::helpers::nonce_account_builder::NonceAccountBuilder,
    mollusk_svm::Mollusk,
    solana_account::Account,
    solana_address::Address,
    spl_programmatic_signer_core::{instruction::initialize_nonce, state::Nonce},
};

pub const MOCK_PROGRAM_ID: Address =
    solana_address::address!("MockProgram11111111111111111111111111111111");

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(&MOCK_PROGRAM_ID, "core_mock_program")
}

pub fn decode_state(account: &Account) -> Nonce {
    wincode::deserialize_exact(&account.data).unwrap()
}

pub fn initialize_nonce_account(mollusk: &Mollusk, authority: &Address) -> (Address, Account) {
    initialize_nonce_account_at(mollusk, authority, Address::new_unique())
}

pub fn initialize_nonce_account_at(
    mollusk: &Mollusk,
    authority: &Address,
    nonce_address: Address,
) -> (Address, Account) {
    let (nonce_account_address, nonce_account) =
        NonceAccountBuilder::new().key(nonce_address).build();
    let instruction = initialize_nonce(&MOCK_PROGRAM_ID, &nonce_account_address, authority);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            (nonce_account_address, nonce_account),
            (*authority, Account::default()),
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[mollusk_svm::result::Check::success()],
    );

    (
        nonce_account_address,
        result.get_account(&nonce_account_address).unwrap().clone(),
    )
}
