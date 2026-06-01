//! Shared wire types for the SPL Message Executor program.
#![no_std]

extern crate alloc;

pub mod error;
pub mod instruction;
pub mod message;

solana_address::declare_id!("ExecmB3YutyrSMtdGpscX9WyFcqWTJ17LrZrFNBpnkx7");
