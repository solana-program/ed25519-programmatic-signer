//! Wire-format types for offline-authorized signed messages submitted via `Submit`.
//!
//! Flow:
//! 1. Build a [`SignedMessage`].
//! 2. Serialize it with `wincode`.
//! 3. Have authority-policy members sign those exact serialized [`SignedMessage`]
//!    bytes offline using Ed25519.
//! 4. Construct [`InstructionData`] from the signatures and message.
//! 5. Submit that payload via [`NonceInstruction::Submit`](crate::instruction::NonceInstruction::Submit).
//!
//! The signatures cover only the serialized [`SignedMessage`], not the outer
//! Solana transaction. The transaction is just the transport that carries the
//! signed message to the program.
//!
//! ## Wire layout
//!
//! ```text
//! InstructionData
//! ┌───────────────┬────────────────────────┬────────────────────────┐
//! │ discriminator │ signatures             │ message                │
//! │ u8            │ count:u8 + entries     │ SignedMessage          │
//! └───────────────┴────────────────────────┴────────────────────────┘
//!
//! SignedMessage
//! ┌─────────┬──────────────────┬────────────────────────────────────┐
//! │ version │ header           │ action                             │
//! │ u8      │ MessageHeader    │ SignedAction                       │
//! └─────────┴──────────────────┴────────────────────────────────────┘
//!
//! MessageHeader
//! ┌──────────────┬──────────────────────────────┐
//! │ nonce        │ deadline                     │
//! │ u32 LE       │ i64 LE (0 = no expiration)   │
//! └──────────────┴──────────────────────────────┘
//! ```
//!
//! ### `SignedAction::Execute`
//!
//! ```text
//! ┌──────────────────────┬──────────────────────────────┐
//! │ account_table        │ instructions                 │
//! │ count:u8 + addresses │ count:u8 + CpiInstructions   │
//! └──────────────────────┴──────────────────────────────┘
//! ```
//!
//! The `account_table` is the signed list of addresses that CPI instructions
//! reference by index. When the transaction is submitted, the caller must pass
//! those same addresses as remaining accounts on the `Submit` instruction, in
//! the same order:
//!
//! ```text
//! Submit accounts:
//!   [0] NonceStatePda (always first)
//!   [1] account_table[0]
//!   [2] account_table[1]
//!   [3] account_table[2]
//!   ...
//! ```
//!
//! The program checks that the submitted accounts match the signed table
//! exactly, preventing account substitution by the submitter.
//!
//! ### `SignedAction::AdvanceNonce`
//!
//! No payload.
//!
//! This action consumes the current nonce and increments it, invalidating all
//! previously signed messages for the account.
//!
//! ### `SignedAction::Close`
//!
//! ```text
//! ┌─────────────────────┐
//! │ recipient: Address  │
//! └─────────────────────┘
//! ```
//!
//! The signed message specifies which address receives the lamports when the
//! nonce state account is closed.

use {
    alloc::vec::Vec,
    solana_address::Address,
    solana_signature::SIGNATURE_BYTES,
    solana_zero_copy::unaligned::{I64, U32},
    wincode::{SchemaRead, SchemaWrite, containers},
};

/// Current signed-message format version.
pub const SIGNED_MESSAGE_VERSION: u8 = 1;

/// Serialized size of [`MessageHeader`] in bytes.
pub const HEADER_LEN: usize = 12;

/// Full instruction-data body passed to
/// [`NonceInstruction::Submit`](crate::instruction::NonceInstruction::Submit).
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
pub struct InstructionData {
    /// Must be `NonceInstruction::Submit` (1).
    pub discriminator: u8,
    /// Ed25519 signatures over the serialized [`InstructionData::message`].
    #[wincode(with = "containers::Vec<SignatureEntry, u8>")]
    pub signatures: Vec<SignatureEntry>,
    /// The exact value authority-policy members sign. Contains the nonce,
    /// deadline, and action the program verifies and executes.
    pub message: SignedMessage,
}

/// One authority-member approval attached to [`InstructionData`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct SignatureEntry {
    /// Index into [`AuthorityPolicy::members`](crate::state::AuthorityPolicy::members).
    pub signer_index: u8,
    /// Ed25519 signature over the serialized [`SignedMessage`].
    pub signature: [u8; SIGNATURE_BYTES],
}

/// The message authority-policy members approve offline.
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
pub struct SignedMessage {
    /// Format version. Must be [`SIGNED_MESSAGE_VERSION`].
    pub version: u8,
    /// Replay-protection header containing the expected nonce and optional
    /// deadline.
    pub header: MessageHeader,
    /// The exact action the authority approved.
    pub action: SignedAction,
}

/// Fixed-size replay-protection header for a [`SignedMessage`].
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
#[wincode(assert_zero_copy)]
pub struct MessageHeader {
    /// Expected nonce value. Must match the nonce stored in the state account.
    pub nonce: U32,
    /// Unix timestamp after which the message expires.
    /// Zero means the message does not expire.
    pub deadline: I64,
}

const _: () = assert!(core::mem::size_of::<MessageHeader>() == HEADER_LEN);

/// Every post-initialization operation the authority can approve goes through
/// one of these variants.
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum SignedAction {
    /// Execute the signed CPI sequence.
    Execute {
        /// Account addresses referenced by CPI instructions. The program checks
        /// that this table matches the `Submit` instruction's remaining
        /// accounts in order, and CPI instructions reference this table by index.
        #[wincode(with = "containers::Vec<Address, u8>")]
        account_table: Vec<Address>,
        /// CPI instructions to execute in order. Each instruction references
        /// its program and accounts by index into
        /// [`SignedAction::Execute`]'s `account_table`.
        #[wincode(with = "containers::Vec<CpiInstruction, u8>")]
        instructions: Vec<CpiInstruction>,
    },
    /// Increment the nonce without executing any CPI, invalidating all
    /// previously signed messages for the account.
    AdvanceNonce,
    /// Close the nonce state account and refund its lamports.
    Close {
        /// Address that receives all lamports from the closed account.
        recipient: Address,
    },
}

/// A CPI instruction authorized by [`SignedAction::Execute`].
#[derive(Clone, Debug, PartialEq, SchemaRead, SchemaWrite)]
pub struct CpiInstruction {
    /// Index into [`SignedAction::Execute`]'s `account_table` for the target
    /// program to invoke.
    pub program_id_index: u8,
    /// Per-account metadata for the CPI.
    #[wincode(with = "containers::Vec<AccountMeta, u8>")]
    pub accounts: Vec<AccountMeta>,
    /// Raw instruction data passed to the target program.
    #[wincode(with = "containers::Vec<u8, u16>")]
    pub data: Vec<u8>,
}

/// An account passed to a CPI, identified by its position in the signed
/// account table along with the signer and writable privileges the authority
/// approved for it.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct AccountMeta {
    /// Position of this account in [`SignedAction::Execute`]'s `account_table`.
    pub account_index: u8,
    /// Whether the authority approved this account as a signer for the CPI.
    pub is_signer: bool,
    /// Whether the authority approved this account as writable for the CPI.
    pub is_writable: bool,
}
