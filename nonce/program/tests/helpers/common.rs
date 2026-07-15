use {
    mollusk_svm::Mollusk, solana_account::Account,
    spl_ed25519_programmatic_signer_legacy_interface::state::SignerContext,
};

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(
        &spl_ed25519_programmatic_signer_legacy_interface::id(),
        "spl_ed25519_programmatic_signer_legacy_program",
    )
}

pub fn decode_state(account: &Account) -> SignerContext {
    wincode::deserialize_exact(&account.data).unwrap()
}
