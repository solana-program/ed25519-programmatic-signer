use {
    solana_program_error::ProgramError,
    solana_transaction::versioned::VersionedTransaction,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Ed25519 Signer program.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum Instruction {
    /// Verifies authority signatures over a Solana `VersionedTransaction`, then CPIs to the program
    /// of its single executor instruction, promoting `ProgrammaticSigner` PDAs to a signer.
    ///
    /// Instruction data: instruction discriminator followed by a serialized `Submit(VersionedTransaction)`.
    ///
    /// On success, the program:
    /// 1. Verifies the message contains exactly one executor instruction.
    /// 2. Verifies each `signatures[i]` is `account_keys[i]`'s Ed25519 signature over that message.
    /// 3. Verifies submitted account keys match the message's account keys in order.
    /// 4. CPIs to the executor instruction's program using exactly the accounts referenced by the
    ///    executor instruction's account index list, promoting any referenced authority-derived
    ///    `ProgrammaticSigner` PDAs to a signer.
    ///
    /// Trust assumptions:
    /// - This program only validates authority signatures, accounts, and flags within wrapped transaction.
    /// - The inner signed transaction is an authorization envelope. Only the single executor
    ///   instruction is invoked.
    /// - The executor instruction data is opaque to this program.
    /// - This program is stateless. Replay protection belongs to the executor program.
    ///
    /// Accounts required:
    /// - One account for each key in the wrapped message's `account_keys` list, in the same order.
    ///   At every index, the submitted account key and writable flag must match the wrapped message.
    ///   V0 address table lookups are not resolved. The executor instruction must reference only
    ///   static `account_keys` indices.
    Submit(VersionedTransaction),
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
        super::Instruction, solana_program_error::ProgramError,
        solana_transaction::versioned::VersionedTransaction,
    };

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(
            wincode::serialize(&Instruction::Submit(VersionedTransaction::default())).unwrap()[0],
            0
        );
    }

    #[test]
    fn submit_round_trips() {
        let instruction = Instruction::Submit(VersionedTransaction::default());
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);
    }

    #[test]
    fn submit_rejects_trailing_data() {
        let mut bytes =
            wincode::serialize(&Instruction::Submit(VersionedTransaction::default())).unwrap();
        bytes.extend_from_slice(&[1, 2, 3]);

        assert_eq!(
            Instruction::try_from_bytes(&bytes),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test]
    fn try_from_bytes_rejects_unknown() {
        assert_eq!(
            Instruction::try_from_bytes(&[1]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            Instruction::try_from_bytes(&[255]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
