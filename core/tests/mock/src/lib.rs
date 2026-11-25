#![no_std]

extern crate alloc;

#[path = "../../helpers/mock_auth.rs"]
mod auth;
mod entrypoint;
mod scheme;
