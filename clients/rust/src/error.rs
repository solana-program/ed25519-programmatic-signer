//! Error type for Rust helper construction and verification.

use {core::fmt, solana_address::Address};

/// Result type returned by these Rust helpers.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors returned by Rust helper construction, signing, serialization, and verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A transaction plan must name at least one cold authority.
    EmptyAuthorities,
    /// A signer or account key appeared more than once where uniqueness is required.
    DuplicateAddress(Address),
    /// A message needs more required signers than the legacy header can encode.
    TooManySigners(usize),
    /// A message needs more account keys than compiled instruction indexes can encode.
    TooManyAccountKeys(usize),
    /// The message compiler rejected an instruction account key.
    MessageCompilation,
    /// A signer failed to produce a public key or signature.
    SignerFailure,
    /// The supplied signer is not required by the transaction.
    SignerNotRequired(Address),
    /// Two transactions do not wrap the same signed message.
    TransactionMismatch,
    /// Two transactions carry different signatures for the same required signer.
    SignatureConflict(Address),
    /// A signature slot contains bytes that do not verify for its signer.
    InvalidSignature(Address),
    /// A submit transaction requires a fully signed transaction.
    NotFullySigned,
    /// A submit transaction is missing a required outer signer.
    MissingOuterSigner(Address),
    /// Transaction serialization failed.
    SerializeTransaction,
    /// Transaction deserialization failed.
    DeserializeTransaction,
    /// JSON input could not be decoded.
    InvalidJson,
    /// The transaction field is not valid standard base64.
    InvalidBase64,
    /// Sign-only JSON did not include a dumped transaction message.
    MissingTransactionMessage,
    /// Sign-only JSON blockhash does not match the dumped message's lifetime specifier.
    SignOnlyLifetimeMismatch,
    /// A required inner signer cannot be supplied by an authority PDA or live submit signer.
    RequiredInnerSignerNotCovered(Address),
    /// A submit signer must already be a required signer of the inner message.
    SubmitSignerNotRequired(Address),
    /// A submit signer cannot be a ProgrammaticSigner PDA because PDAs cannot sign the cold-signed transaction.
    SubmitSignerCannotBeProgrammaticSigner(Address),
    /// Sign-only JSON reported at least one bad signature.
    BadSignOnlySignatures,
    /// Nonce account data could not be decoded.
    InvalidNonceAccount,
    /// The inner message lifetime specifier does not match the stored nonce.
    NonceMismatch,
    /// The cold-signed transaction genesis hash does not match the expected cluster genesis hash.
    GenesisHashMismatch,
    /// The cold-signed transaction is not valid.
    InvalidWrappedTransaction,
    /// The inner message is not valid.
    InvalidInnerMessage,
    /// Address lookup tables are not supported by the cold-signed transaction model.
    AddressLookupTablesUnsupported,
    /// The inner message contains a duplicate static account key.
    DuplicateMessageAccount(Address),
    /// The nonce authority is not a required signer of the inner message.
    MissingNonceAuthority,
    /// The cold-signed transaction must contain exactly one executor instruction.
    InvalidExecutorInstructionCount,
    /// The executor instruction does not target the configured executor program.
    InvalidExecutorProgramId,
    /// The executor instruction does not pass the configured nonce program.
    InvalidNonceProgramId,
    /// The nonce authority is not authorized by the configured signer program.
    InvalidSignerProgramId,
    /// The signer program cannot provide a required inner signer privilege.
    MissingSignerPrivilege(Address),
    /// The wrapper does not provide required writable privilege to the executor.
    MissingWritablePrivilege(Address),
    /// Instruction data did not decode to the expected interface instruction.
    InvalidInstructionData,
    /// The executor instruction does not reference the expected nonce account.
    InvalidNonceAccountMeta,
    /// An outer signer was not required by the submit instruction.
    OuterSignerNotRequired(Address),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAuthorities => {
                formatter.write_str("transaction plan requires at least one authority")
            }
            Self::DuplicateAddress(address) => {
                write!(formatter, "duplicate address {address}")
            }
            Self::TooManySigners(count) => write!(formatter, "too many required signers {count}"),
            Self::TooManyAccountKeys(count) => write!(formatter, "too many account keys {count}"),
            Self::MessageCompilation => formatter.write_str("message compilation failed"),
            Self::SignerFailure => formatter.write_str("signer operation failed"),
            Self::SignerNotRequired(address) => {
                write!(formatter, "signer {address} is not required")
            }
            Self::TransactionMismatch => formatter.write_str("transactions do not match"),
            Self::SignatureConflict(address) => {
                write!(formatter, "conflicting signature for {address}")
            }
            Self::InvalidSignature(address) => {
                write!(formatter, "invalid signature for {address}")
            }
            Self::NotFullySigned => formatter.write_str("transaction is not fully signed"),
            Self::MissingOuterSigner(address) => {
                write!(formatter, "missing outer signer {address}")
            }
            Self::SerializeTransaction => formatter.write_str("transaction serialization failed"),
            Self::DeserializeTransaction => {
                formatter.write_str("transaction deserialization failed")
            }
            Self::InvalidJson => formatter.write_str("invalid JSON"),
            Self::InvalidBase64 => formatter.write_str("invalid base64 transaction"),
            Self::MissingTransactionMessage => {
                formatter.write_str("missing dumped transaction message")
            }
            Self::SignOnlyLifetimeMismatch => formatter
                .write_str("sign-only blockhash does not match the message lifetime specifier"),
            Self::RequiredInnerSignerNotCovered(address) => {
                write!(formatter, "required inner signer {address} is not covered")
            }
            Self::SubmitSignerNotRequired(address) => {
                write!(
                    formatter,
                    "submit signer {address} is not required by inner message"
                )
            }
            Self::SubmitSignerCannotBeProgrammaticSigner(address) => {
                write!(
                    formatter,
                    "submit signer {address} is a ProgrammaticSigner PDA"
                )
            }
            Self::BadSignOnlySignatures => {
                formatter.write_str("sign-only output contains bad signatures")
            }
            Self::InvalidNonceAccount => formatter.write_str("invalid nonce account data"),
            Self::NonceMismatch => formatter.write_str("nonce mismatch"),
            Self::GenesisHashMismatch => formatter.write_str("genesis hash mismatch"),
            Self::InvalidWrappedTransaction => formatter.write_str("invalid wrapped transaction"),
            Self::InvalidInnerMessage => formatter.write_str("invalid inner message"),
            Self::AddressLookupTablesUnsupported => {
                formatter.write_str("address lookup tables are not supported")
            }
            Self::DuplicateMessageAccount(address) => {
                write!(formatter, "duplicate message account {address}")
            }
            Self::MissingNonceAuthority => formatter.write_str("missing nonce authority signer"),
            Self::InvalidExecutorInstructionCount => {
                formatter.write_str("invalid executor instruction count")
            }
            Self::InvalidExecutorProgramId => formatter.write_str("invalid executor program id"),
            Self::InvalidNonceProgramId => formatter.write_str("invalid nonce program id"),
            Self::InvalidSignerProgramId => formatter.write_str("invalid signer program id"),
            Self::MissingSignerPrivilege(address) => {
                write!(formatter, "missing signer privilege for {address}")
            }
            Self::MissingWritablePrivilege(address) => {
                write!(formatter, "missing writable privilege for {address}")
            }
            Self::InvalidInstructionData => formatter.write_str("invalid instruction data"),
            Self::InvalidNonceAccountMeta => formatter.write_str("invalid nonce account meta"),
            Self::OuterSignerNotRequired(address) => {
                write!(formatter, "outer signer {address} is not required")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<solana_signer::SignerError> for Error {
    fn from(_error: solana_signer::SignerError) -> Self {
        Self::SignerFailure
    }
}
