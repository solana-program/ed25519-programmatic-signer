#![no_std]

#[cfg(any(feature = "client", test))]
extern crate alloc;

pub mod error;
pub mod instruction;
#[cfg(feature = "program")]
mod nonce;
#[cfg(feature = "program")]
pub mod processor;
pub mod state;

#[cfg(feature = "program")]
pub use processor::process_instruction;
