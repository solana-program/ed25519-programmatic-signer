//! Durable Signer program.
//!
//! The deployed binary selects one signing scheme at compile time. Default
//! builds use native Solana Ed25519 transactions; `--features falcon` builds the
//! same nonce/PDA/CPI processor with the Falcon-512 signing scheme.

#![no_std]

extern crate alloc;

mod config;
mod entrypoint;
#[cfg(feature = "falcon")]
mod falcon;
mod initialize;
mod processor;
mod submit;
mod verifier;
