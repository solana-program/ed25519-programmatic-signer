use {
    solana_hash::Hash,
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Nonce program.
///
/// Wire tags are pinned per variant with `#[wincode(tag = ...)]`. Wincode ignores Rust
/// enum discriminants, so explicit tags keep the format stable under reordering.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum Instruction {
    /// Initializes a nonce account for an authority.
    ///
    /// The caller must first create and fund the account. Recommended to include
    /// the system create-account instruction and `Initialize` in the same transaction so no
    /// other transaction can initialize the account first.
    ///
    /// On success the program does the following.
    /// 1. Verifies the account is uninitialized, rent-exempt, and owned by this program.
    /// 2. Derives the initial `nonce` by hashing the nonce derivation tag, nonce account
    ///    address, and latest slot hash.
    /// 3. Writes `Nonce { nonce, authority }` into the account data.
    ///
    /// Instruction data is the discriminator only.
    ///
    /// Required accounts.
    /// - `[writable]` Nonce account
    /// - `[]` Authority to store in the nonce account
    /// - `[]` `SlotHashes` sysvar
    Initialize,

    /// Consumes the stored nonce and advances it to a fresh value.
    ///
    /// Consumers verify the stored nonce by reading the account, then CPI this
    /// instruction after their work succeeds. `current_nonce` is re-checked here so the
    /// nonce cannot be consumed twice within one transaction.
    ///
    /// Instruction data is the discriminator followed by a serialized
    /// [`AdvanceNonce`].
    ///
    /// On success the program does the following.
    /// 1. Verifies the stored authority matches the authority account, which must carry
    ///    runtime signer privilege.
    /// 2. Verifies the stored nonce equals `current_nonce`.
    /// 3. Stores the next nonce by hashing the nonce derivation tag, nonce account,
    ///    old nonce, and latest slot hash.
    ///
    /// Required accounts.
    /// - `[writable]` Nonce account
    /// - `[signer]` Authority stored in the nonce account
    /// - `[]` `SlotHashes` sysvar
    Advance(AdvanceNonce),

    /// Reserved close instruction.
    ///
    /// Instruction data is the discriminator only.
    ///
    /// This is not implemented yet. The current processor rejects it with an invalid
    /// instruction data error. A future implementation should require the stored authority's
    /// signer privilege.
    ///
    /// Required accounts.
    /// - `[writable]` Nonce account
    /// - `[signer]` Authority stored in the nonce account
    /// - `[writable]` Lamport recipient
    Close,
}

/// Payload for nonce advancement.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct AdvanceNonce {
    /// The nonce value the caller verified before doing its work.
    pub current_nonce: Hash,
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
    use {
        super::{AdvanceNonce, Hash, Instruction},
        solana_program_error::ProgramError,
    };

    fn advance() -> Instruction {
        Instruction::Advance(AdvanceNonce {
            current_nonce: Hash::new_from_array([1; 32]),
        })
    }

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(wincode::serialize(&Instruction::Initialize).unwrap()[0], 0);
        assert_eq!(wincode::serialize(&advance()).unwrap()[0], 1);
        assert_eq!(wincode::serialize(&Instruction::Close).unwrap()[0], 2);
    }

    #[test]
    fn advance_round_trips() {
        let bytes = wincode::serialize(&advance()).unwrap();
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), advance());
    }

    #[test]
    fn try_from_bytes_rejects_unknown() {
        assert_eq!(
            Instruction::try_from_bytes(&[3]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            Instruction::try_from_bytes(&[255]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            Instruction::try_from_bytes(&[]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
