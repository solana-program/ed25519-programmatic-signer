use {crate::helpers::stub_executor, mollusk_svm::Mollusk};

pub fn init_mollusk() -> Mollusk {
    let mut mollusk = Mollusk::new(
        &spl_ed25519_signer_interface::id(),
        "spl_ed25519_signer_program",
    );
    stub_executor::install(&mut mollusk);
    mollusk
}
