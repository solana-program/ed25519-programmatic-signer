//! Ed25519 Durable Signer program.

#![no_std]

extern crate alloc;

mod config;
mod entrypoint;
pub mod falcon;
mod initialize;
mod processor;
mod submit;
pub mod verifier;
