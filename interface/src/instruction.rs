use {
    alloc::vec::Vec,
    solana_program_error::ProgramError,
    solana_transaction::{VersionedMessage, versioned::VersionedTransaction},
    wincode::{SchemaRead, SchemaWrite, config::DefaultConfig},
};

/// Falcon-512 public key length, in bytes.
pub const FALCON512_PUBLIC_KEY_LEN: usize = 897;
/// Falcon-512 compressed signature length after zero-padding, in bytes.
pub const FALCON512_SIGNATURE_LEN: usize = 666;

/// Falcon-512 public key supplied when initializing a Falcon durable signer.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct FalconInitialize {
    /// Standard Falcon-512 wire public key with a header byte and packed polynomial.
    pub public_key: [u8; FALCON512_PUBLIC_KEY_LEN],
}

/// Falcon-512 signature for one wrapped required signer.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct FalconSignature {
    /// Standard compressed Falcon-512 signature (`0x39` header) padded with
    /// trailing zeroes to the fixed verification input size.
    pub bytes: [u8; FALCON512_SIGNATURE_LEN],
}

impl AsRef<[u8]> for FalconSignature {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl FalconSignature {
    /// Builds a fixed-size Falcon signature from a compressed signature.
    ///
    /// `solana-falcon512` expects a fixed 666-byte buffer. PQClean-style
    /// compressed signatures can be shorter, so callers pass the compressed
    /// bytes and this constructor right-pads them with zeroes.
    pub fn try_from_compressed(compressed: &[u8]) -> Result<Self, ProgramError> {
        if compressed.len() > FALCON512_SIGNATURE_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut bytes = [0u8; FALCON512_SIGNATURE_LEN];
        bytes[..compressed.len()].copy_from_slice(compressed);
        Ok(Self { bytes })
    }
}

/// Falcon-specific signed envelope for `Submit`.
///
/// `VersionedTransaction` cannot represent Falcon directly because its
/// signature vector is fixed to 64-byte Solana signatures. The Falcon program
/// variant therefore keeps the reusable [`VersionedMessage`] and pairs it with
/// Falcon-sized signatures.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct FalconSubmit {
    /// Falcon signatures ordered by required signer index.
    ///
    /// This first Falcon implementation requires exactly one signature because
    /// the durable signer state stores one Falcon public key. The vector keeps
    /// the large signature bytes off the processor stack during instruction
    /// deserialization.
    pub signatures: Vec<FalconSignature>,
    /// Wrapped message whose required signer key is the durable signer PDA.
    pub message: VersionedMessage,
}

/// Generic instruction envelope used by concrete program variants.
///
/// The Ed25519 deployment uses [`DurableSignerInstruction`] below, preserving
/// `Submit(VersionedTransaction)` for native Solana tooling. The Falcon
/// deployment uses [`FalconDurableSignerInstruction`], where `Submit` carries a
/// Falcon-specific signed envelope instead.
#[repr(u8)]
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum DurableSignerInstructionData<Initialize, Submit> {
    /// Initializes a durable signer account for an authority.
    ///
    /// The caller must first create and fund the account. Recommended to include
    /// `solana_system_interface::instruction::create_account` and `Initialize` in the same
    /// transaction so no other transaction can initialize the account first.
    ///
    /// On success, the program:
    /// 1. Verifies the account is uninitialized, rent-exempt, and owned by this program.
    /// 2. Derives the initial `nonce` as
    ///    `sha256("spl-ed25519-durable-signer::init-v1" ‖ durable_signer_account_address ‖ slot_hashes[0])`.
    /// 3. Writes `DurableSignerAccount { nonce, authority }` into the account data.
    ///
    /// Instruction data: instruction discriminator followed by the
    /// scheme-specific initialize payload. The Ed25519 payload is `()`, which
    /// serializes to no bytes and keeps the original one-byte instruction.
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account
    /// - Scheme-specific authority inputs
    /// - `[]` `SlotHashes` sysvar
    Initialize(Initialize),

    /// Authorizes and executes a wrapped Solana transaction whose required signers are
    /// `DurableSignerPda` accounts.
    ///
    /// Instruction data: instruction discriminator followed by a serialized
    /// scheme-specific signed envelope. Ed25519 uses
    /// [`VersionedTransaction`]; Falcon uses [`FalconSubmit`].
    ///
    /// Wrapped required signers are paired by index:
    /// - `message.account_keys[i]`: `DurableSignerPda` promoted during CPI.
    /// - The active scheme's submit envelope: wrapped-message authorization in
    ///   that scheme's signature format.
    ///
    /// On success, the program:
    /// 1. Deserializes the transaction and sanitizes the wrapped message.
    /// 2. Reads the authority stored in the durable signer account.
    /// 3. Checks the passed durable signer account's authority signed the wrapped message.
    /// 4. Checks the wrapped message's lifetime / recent blockhash field equals the account's
    ///    `nonce`.
    /// 5. Verifies the outer transaction's only top-level instruction is `Submit`.
    /// 6. Asks the active signing scheme to verify each required signer and
    ///    prove that `message.account_keys[i]` is the corresponding
    ///    `DurableSignerPda`.
    /// 7. Executes each `message.instructions` entry by CPI, using `invoke_signed` to promote
    ///    each authorized signer's corresponding `DurableSignerPda`.
    /// 8. Derives and stores the next nonce as
    ///    `sha256("spl-ed25519-durable-signer::v1" ‖ durable_signer_account ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(signed_message_bytes))`
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account whose nonce is consumed and advanced
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Scheme-specific authority accounts, if the active scheme needs them.
    ///   Ed25519 expects authority addresses ordered to match the wrapped
    ///   message's required signers; the first Falcon variant stores authority
    ///   material in the durable signer account and expects none here.
    /// - Remaining accounts referenced by the wrapped message, in order. Writable flags
    ///   must match the wrapped message.
    Submit(Submit),

    /// Reserved close instruction.
    ///
    /// Instruction data: instruction discriminator only.
    ///
    /// This is not implemented yet; the current processor rejects it with
    /// `ProgramError::InvalidInstructionData`. A future implementation should
    /// run only as an inner instruction of a wrapped transaction submitted
    /// through `Submit`, because nothing outside this program can sign for
    /// `DurableSignerPda`.
    ///
    /// Accounts required:
    /// - `[signer]` `DurableSignerPda`
    /// - `[writable]` Durable signer account
    /// - `[writable]` Lamport recipient
    Close,
}

/// Standard Ed25519 instruction format.
pub type DurableSignerInstruction = DurableSignerInstructionData<(), VersionedTransaction>;

/// Falcon-512 instruction format.
pub type FalconDurableSignerInstruction =
    DurableSignerInstructionData<FalconInitialize, FalconSubmit>;

impl<Initialize, Submit> DurableSignerInstructionData<Initialize, Submit> {
    #[inline(always)]
    pub fn try_from_bytes(instruction_data: &[u8]) -> Result<Self, ProgramError>
    where
        for<'de> Self: SchemaRead<'de, DefaultConfig, Dst = Self>,
    {
        wincode::deserialize_exact(instruction_data)
            .map_err(|_| ProgramError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{DurableSignerInstruction, FALCON512_SIGNATURE_LEN, FalconSignature},
        alloc::vec,
        solana_transaction::{Message, VersionedMessage, versioned::VersionedTransaction},
    };

    fn empty_transaction() -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message::default()),
        }
    }

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(
            wincode::serialize(&DurableSignerInstruction::Initialize(())).unwrap()[0],
            0
        );
        assert_eq!(
            wincode::serialize(&DurableSignerInstruction::Submit(empty_transaction())).unwrap()[0],
            1
        );
        assert_eq!(
            wincode::serialize(&DurableSignerInstruction::Close).unwrap()[0],
            2
        );
    }

    #[test]
    fn try_from_bytes_rejects_unknown() {
        assert!(DurableSignerInstruction::try_from_bytes(&[4]).is_err());
        assert!(DurableSignerInstruction::try_from_bytes(&[255]).is_err());
    }

    #[test]
    fn falcon_signature_constructor_pads_and_rejects_oversized_input() {
        let signature = FalconSignature::try_from_compressed(&[1, 2, 3]).unwrap();

        assert_eq!(&signature.bytes[..3], &[1, 2, 3]);
        assert!(signature.bytes[3..].iter().all(|byte| *byte == 0));
        assert!(
            FalconSignature::try_from_compressed(&vec![0; FALCON512_SIGNATURE_LEN + 1]).is_err()
        );
    }
}
