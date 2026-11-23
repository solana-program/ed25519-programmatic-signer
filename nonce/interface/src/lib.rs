#![no_std]

pub mod error;
pub mod instruction;
pub mod state;

solana_address::declare_id!("Nonce8z7LeGkpQ2bCXtSLdZqREMbuDHGhmf4ELcUU91");

#[cfg(feature = "codama")]
codama_macros::codama_program!(name = "nonce");
