use {
    solana_program_error::ProgramError,
    solana_transaction::versioned::VersionedTransaction,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Ed25519 Durable Signer program.
#[repr(u8)]
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum DurableSignerInstruction {
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
    /// Instruction data: instruction discriminator only
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account
    /// - `[]` Authority to store in the durable signer account
    /// - `[]` `SlotHashes` sysvar
    Initialize,

    /// Authorizes and executes a wrapped Solana transaction whose required signers are
    /// `DurableSignerPda` accounts.
    ///
    /// Instruction data: instruction discriminator followed by a serialized
    /// `solana_transaction::versioned::VersionedTransaction`.
    /// All message variants supported by `VersionedTransaction` are accepted.
    ///
    /// Wrapped required signers are paired by index:
    /// - `message.account_keys[i]`: `DurableSignerPda` promoted during CPI.
    /// - `tx.signatures[i]`: wrapped-message signature from the matching authority address.
    ///
    /// On success, the program:
    /// 1. Deserializes the transaction and sanitizes the wrapped message.
    /// 2. Reads the authority stored in the durable signer account.
    /// 3. Checks the passed durable signer account's authority signed the wrapped message.
    /// 4. Checks the wrapped message's lifetime / recent blockhash field equals the account's
    ///    `nonce`.
    /// 5. Verifies the outer transaction's only top-level instruction is `Submit`.
    /// 6. Iterates over the outer authority accounts in order. For each `authority_i`, requires
    ///    `DurableSignerPda(authority_i) == message.account_keys[i]` and verifies
    ///    `tx.signatures[i]` over the wrapped message with `authority_i`.
    /// 7. Executes each `message.instructions` entry by CPI, using `invoke_signed` to promote
    ///    each authorized signer's corresponding `DurableSignerPda`.
    /// 8. Derives and stores the next nonce as
    ///    `sha256("spl-ed25519-durable-signer::v1" ‖ durable_signer_account ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(signed_message_bytes))`
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account whose nonce is consumed and advanced
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Authority addresses, ordered to match the wrapped message's required signers.
    /// - Remaining accounts referenced by the wrapped message, in order. Writable flags
    ///   must match the wrapped message.
    Submit(VersionedTransaction),

    /// Closes a durable signer account and refunds its lamports.
    ///
    /// Instruction data: instruction discriminator only.
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`
    /// because nothing outside this program can sign for `DurableSignerPda`.
    ///
    /// Accounts required:
    /// - `[signer]` `DurableSignerPda`
    /// - `[writable]` Durable signer account
    /// - `[writable]` Lamport recipient
    Close,
}

impl DurableSignerInstruction {
    #[inline(always)]
    pub fn try_from_bytes(instruction_data: &[u8]) -> Result<Self, ProgramError> {
        wincode::deserialize_exact(instruction_data)
            .map_err(|_| ProgramError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::DurableSignerInstruction,
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
            wincode::serialize(&DurableSignerInstruction::Initialize).unwrap()[0],
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
}
