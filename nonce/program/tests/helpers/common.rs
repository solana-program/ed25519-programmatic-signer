use {
    crate::helpers::nonce_account_builder::NonceAccountBuilder, mollusk_svm::Mollusk,
    solana_account::Account, solana_address::Address, spl_nonce_client::instruction::initialize,
    spl_nonce_interface::state::Nonce,
};

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(&spl_nonce_interface::id(), "spl_nonce_program")
}

pub fn decode_state(account: &Account) -> Nonce {
    wincode::deserialize_exact(&account.data).unwrap()
}

pub fn initialize_nonce_account(mollusk: &Mollusk, authority: &Address) -> (Address, Account) {
    let (nonce_account_address, nonce_account) = NonceAccountBuilder::new()
        .key(Address::new_unique())
        .build();
    let instruction = initialize(&nonce_account_address, authority);
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
