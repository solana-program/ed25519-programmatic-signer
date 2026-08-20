#![no_std]

pub mod error;
pub mod instruction;
pub mod state;

solana_address::declare_id!("Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB");

#[cfg(feature = "codama")]
codama_macros::codama_program!(name = "nonce");
