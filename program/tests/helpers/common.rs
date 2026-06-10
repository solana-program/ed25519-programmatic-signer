use {
    mollusk_svm::Mollusk, solana_account::Account,
    spl_ed25519_programmatic_signer_interface::state::SignerNonceAccount,
};

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(
        &spl_ed25519_programmatic_signer_interface::id(),
        "spl_ed25519_programmatic_signer_program",
    )
}

pub fn decode_state(account: &Account) -> SignerNonceAccount {
    wincode::deserialize_exact(&account.data).unwrap()
}
