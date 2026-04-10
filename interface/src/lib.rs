//! Interface for the Nonce program.
#![no_std]

extern crate alloc;

pub mod instruction;
pub mod message;
pub mod pda;
pub mod state;

solana_address::declare_id!("nonce34S3Viw97xQwWGpRWEGufiSpfVEAiFe7Lefv7y");
