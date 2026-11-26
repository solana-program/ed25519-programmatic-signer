#![no_std]

extern crate alloc;

pub mod error;
pub mod instruction;
#[cfg(feature = "client")]
pub mod message;
#[cfg(feature = "program")]
mod nonce;
pub mod pda;
#[cfg(feature = "program")]
pub mod processor;
pub mod state;

#[cfg(feature = "program")]
pub use processor::process_instruction;
