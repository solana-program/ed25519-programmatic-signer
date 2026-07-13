//! Interface for the Ed25519 Programmatic Signer program.
#![no_std]

pub mod instruction;
pub mod pda;
pub mod state;

solana_address::declare_id!("EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN");
