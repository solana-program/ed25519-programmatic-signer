//! Interface for the Ed25519 Durable Signer program.
#![no_std]

pub mod instruction;
pub mod pda;
pub mod state;

solana_address::declare_id!("EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN");
