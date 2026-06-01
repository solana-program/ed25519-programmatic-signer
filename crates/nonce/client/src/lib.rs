//! Off-chain client helpers for the SPL Nonce program.
#![no_std]

extern crate alloc;

#[cfg(feature = "cpi")]
pub mod cpi;
pub mod instruction;
