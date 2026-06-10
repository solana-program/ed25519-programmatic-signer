use mollusk_svm::Mollusk;

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(
        &spl_ed25519_signer_interface::id(),
        "spl_ed25519_signer_program",
    )
}
