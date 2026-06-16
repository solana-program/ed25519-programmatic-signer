use {
    solana_address::Address,
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Ed25519 Programmatic Signer program.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum Instruction {
    /// Initializes a signer context account for an authority.
    ///
    /// The caller must first create and fund the account. Recommended to include
    /// `solana_system_interface::instruction::create_account` and `Initialize` in the same
    /// transaction so no other transaction can initialize the account first.
    ///
    /// On success, the program:
    /// 1. Verifies the account is uninitialized, rent-exempt, and owned by this program.
    /// 2. Derives the initial `nonce` as
    ///    `sha256("spl-ed25519-programmatic-signer::init-v1" ‖ signer_context_address ‖ slot_hashes[0])`.
    /// 3. Writes `SignerContext { nonce, authority }` into the account data.
    ///
    /// Instruction data: instruction discriminator only
    ///
    /// Accounts required:
    /// - `[writable]` Signer context account
    /// - `[]` Authority to store in the signer context account
    /// - `[]` `SlotHashes` sysvar
    Initialize,

    /// Authorizes and executes a wrapped Solana transaction whose required signers are
    /// `ProgrammaticSigner` accounts.
    ///
    /// Instruction data: instruction discriminator followed by a serialized
    /// `solana_transaction::versioned::VersionedTransaction`.
    /// All message variants supported by `VersionedTransaction` are accepted.
    ///
    /// Wrapped required signers are paired by index:
    /// - `message.account_keys[i]`: `ProgrammaticSigner` promoted during CPI.
    /// - `tx.signatures[i]`: wrapped-message signature from the matching authority address.
    ///
    /// On success, the program:
    /// 1. Deserializes the transaction and sanitizes the wrapped message.
    /// 2. Reads the authority stored in the signer context account.
    /// 3. Checks the passed signer context account's authority signed the wrapped message.
    /// 4. Checks the wrapped message's lifetime / recent blockhash field equals the account's
    ///    `nonce`.
    /// 5. Verifies the outer transaction's only top-level instruction is `Submit`.
    /// 6. Iterates over the outer authority accounts in order. For each `authority_i`, requires
    ///    `ProgrammaticSigner(authority_i) == message.account_keys[i]` and verifies
    ///    `tx.signatures[i]` over the wrapped message with `authority_i`.
    /// 7. Executes each `message.instructions` entry by CPI, using `invoke_signed` to promote
    ///    each authorized signer's corresponding `ProgrammaticSigner`.
    /// 8. Derives and stores the next nonce as
    ///    `sha256("spl-ed25519-programmatic-signer::v1" ‖ signer_context ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(signed_message_bytes))`
    ///
    /// Accounts required:
    /// - `[writable]` Signer context account whose nonce is consumed and advanced
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Authority addresses, ordered to match the wrapped message's required signers.
    /// - Remaining accounts referenced by the wrapped message, in order. Writable flags
    ///   must match the wrapped message.
    Submit,

    /// Closes a signer context account and refunds its lamports.
    ///
    /// Instruction data: instruction discriminator followed by [`CloseData`].
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`
    /// because nothing outside this program can sign for `ProgrammaticSigner`.
    ///
    /// Accounts required:
    /// - `[signer]` `ProgrammaticSigner`
    /// - `[writable]` Signer context account
    /// - `[writable]` Lamport recipient
    Close,
}

/// Data for [`Instruction::Close`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct CloseData {
    /// Address that receives all lamports from the closed signer context account.
    pub recipient: Address,
}

impl Instruction {
    #[inline(always)]
    pub fn try_from_bytes(instruction_data: &[u8]) -> Result<Self, ProgramError> {
        wincode::deserialize_exact(instruction_data)
            .map_err(|_| ProgramError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use super::Instruction;

    fn instruction_bytes(instruction: Instruction) -> [u8; 1] {
        let mut bytes = [0];
        wincode::serialize_into(bytes.as_mut_slice(), &instruction).unwrap();
        bytes
    }

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(instruction_bytes(Instruction::Initialize), [0]);
        assert_eq!(instruction_bytes(Instruction::Submit), [1]);
        assert_eq!(instruction_bytes(Instruction::Close), [2]);
    }

    #[test]
    fn try_from_bytes_rejects_unknown() {
        assert!(Instruction::try_from_bytes(&[4]).is_err());
        assert!(Instruction::try_from_bytes(&[255]).is_err());
    }
}
