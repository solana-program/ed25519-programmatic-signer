use {
    solana_message::VersionedMessage,
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Message Executor program.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum Instruction {
    /// Executes a wrapped Solana transaction message by invoking each of its instructions via
    /// CPI, consuming a nonce for replay protection.
    ///
    /// This program is intended to be invoked after a signer program has verified signatures and
    /// promoted any authorized PDAs into runtime signer privilege.
    ///
    /// Instruction data is the discriminator followed by a serialized [`VersionedMessage`].
    /// Supported messages are `legacy`, `v0` without address table lookups, and `v1` with empty
    /// transaction config.
    ///
    /// On success the program does the following.
    /// 1. Checks the wrapped message's `recent_blockhash` field equals the nonce account's
    ///    stored `nonce`.
    /// 2. Deserializes and sanitizes the wrapped message, enforcing the replay policy.
    /// 3. Binds each message account to the wrapped message's `account_keys` entry at the
    ///    same index, requiring writable flags to cover the message's writability and signer
    ///    privilege on every required signer.
    /// 4. Checks the nonce account's stored authority is one of the wrapped message's
    ///    required signer account keys.
    /// 5. Executes each `message.instructions` entry by CPI.
    /// 6. CPIs the Nonce program's `Advance`, consuming the nonce.
    ///
    /// Required accounts.
    /// - `[writable]` Nonce account whose nonce is consumed
    /// - `[]` SPL Nonce program
    /// - `[]` `SlotHashes` sysvar, forwarded to `Advance`
    /// - Message accounts referenced by the wrapped message, in order
    Execute(VersionedMessage),
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
        super::Instruction,
        solana_message::{Message, VersionedMessage},
        solana_program_error::ProgramError,
    };

    fn empty_message() -> VersionedMessage {
        VersionedMessage::Legacy(Message::default())
    }

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(
            wincode::serialize(&Instruction::Execute(empty_message())).unwrap()[0],
            0
        );
    }

    #[test]
    fn execute_round_trips() {
        let instruction = Instruction::Execute(empty_message());
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);
    }

    #[test]
    fn execute_rejects_trailing_data() {
        let mut bytes = wincode::serialize(&Instruction::Execute(empty_message())).unwrap();
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
