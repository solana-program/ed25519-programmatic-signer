//! Offline transaction workflows for SPL programmatic signers.
//!
//! Programs and instruction builders live in the per-program crates. This crate composes them
//! into portable, partially signed transaction files without RPC or wallet-specific dependencies.
pub mod error;
pub mod inspect;
mod message;
pub mod nonce;
pub mod sign_only;
pub mod submit;
pub mod transaction;
pub mod transaction_plan;
pub mod verify;

pub use {
    error::{Error, Result},
    inspect::{SignerStatus, TransactionSummary, inspect},
    sign_only::SignOnlyTransaction,
    submit::submit_transaction,
    transaction::{
        build_transaction, is_fully_signed, merge_transactions, sign_transaction, signer_status,
        transaction_from_message, transaction_from_message_checked, transaction_from_sign_only,
        transaction_from_sign_only_checked,
    },
    transaction_plan::TransactionPlan,
    verify::{verify, verify_genesis_hash, verify_static},
};
