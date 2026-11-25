#![no_std]

solana_address::declare_id!("EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN");

#[cfg(feature = "codama")]
codama_macros::codama_program!(name = "ed25519ProgrammaticSigner");
