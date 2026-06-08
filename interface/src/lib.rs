//! Shared wire types for the Durable Signer program.
//!
//! [`instruction::DurableSignerInstruction`] is the default Ed25519 interface.
//! [`instruction::FalconDurableSignerInstruction`] is the Falcon-512 interface
//! used by the `falcon` program build.
#![no_std]

extern crate alloc;

pub mod error;
pub mod instruction;
pub mod pda;
pub mod state;

solana_address::declare_id!("EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN");
