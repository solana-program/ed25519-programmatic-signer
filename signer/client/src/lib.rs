//! Off-chain client helpers for the SPL Ed25519 Signer program.
#![no_std]

extern crate alloc;

pub mod instruction;
pub mod signing;

pub use spl_ed25519_signer_interface::{
    ID, id,
    instruction::{SubmitEnvelope, SubmitPayload},
    pda::ProgrammaticSigner,
};
