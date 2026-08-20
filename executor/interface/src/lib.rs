#![no_std]

pub mod error;
pub mod instruction;

solana_address::declare_id!("ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR");

#[cfg(feature = "codama")]
codama_macros::codama_program!(name = "messageExecutor");
