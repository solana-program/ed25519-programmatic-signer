//! Shared wire types for the SPL Ed25519 Signer program.
#![no_std]

extern crate alloc;

pub mod error;
pub mod instruction;
pub mod pda;

solana_address::declare_id!("EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN");
