#![no_std]

extern crate alloc;

pub mod error;
pub mod instruction;
pub mod pda;
pub mod state;

#[cfg(feature = "client")]
pub mod message;

#[cfg(feature = "program")]
pub mod execute;
#[cfg(feature = "program")]
mod nonce;
#[cfg(feature = "program")]
pub mod processor;
#[cfg(feature = "program")]
pub mod scheme;

#[cfg(feature = "program")]
pub use processor::process_instruction;
