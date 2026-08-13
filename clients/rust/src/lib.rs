//! Rust helpers for the SPL programmatic signer stack.

mod message;

/// Rust helper error type.
pub mod error;
/// Offline cold-signed transaction inspection.
pub mod inspect;
/// Nonce account helpers.
pub mod nonce;
/// Programmatic signer derivation helpers.
pub mod pda;
/// Solana CLI sign-only output import helpers.
pub mod sign_only;
/// Submit transaction assembly.
pub mod submit;
/// Programmatic signer transaction construction and signing.
pub mod transaction;
/// Transaction plan construction helpers.
pub mod transaction_plan;
/// Offline transaction verification.
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
