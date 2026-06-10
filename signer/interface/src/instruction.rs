use {
    alloc::vec::Vec,
    solana_address::Address,
    solana_program_error::ProgramError,
    solana_signature::Signature,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Ed25519 Signer program.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
pub enum Instruction {
    /// Verifies authority signatures over an opaque executor payload, then CPIs to the
    /// executor program promoting each authority's `ProgrammaticSigner` to a signer.
    ///
    /// Instruction data: instruction discriminator followed by a serialized `SubmitEnvelope`.
    ///
    /// On success, the program:
    /// 1. Verifies `payload.signer_program_id` is this program's id.
    /// 2. Verifies each `signatures[i]` is authority account `i`'s Ed25519 signature over
    ///    the serialized `payload`.
    /// 3. Verifies the executor program account matches `payload.executor_program_id`.
    /// 4. Invokes the executor with `payload.executor_instruction_data` and the remaining
    ///    accounts, preserving their privileges and additionally signing for each
    ///    authority's `ProgrammaticSigner`.
    ///
    /// Trust assumptions:
    /// - This program guarantees only that the authorities signed the payload. What the
    ///   payload does is entirely up to the executor it names.
    /// - Signatures cover only the payload, never the accounts. The executor must validate them
    ///   against its instruction data.
    /// - This program is stateless. Replay protection belongs to the executor.
    ///
    /// Accounts required:
    /// - `[]` Authority accounts, one per envelope signature, in order.
    /// - `[]` Executor program
    /// - Remaining accounts forwarded to the executor, in the order it expects.
    Submit(SubmitEnvelope),
}

/// Authority signatures over a `SubmitPayload`, paired by index with the authority
/// accounts passed to `Instruction::Submit`.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct SubmitEnvelope {
    pub signatures: Vec<Signature>,
    pub payload: SubmitPayload,
}

/// What an authority signs: the signer program it targets, the executor to invoke, and
/// the exact instruction data to invoke it with.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct SubmitPayload {
    pub signer_program_id: Address,
    pub executor_program_id: Address,
    pub executor_instruction_data: Vec<u8>,
}

impl SubmitPayload {
    /// The bytes an authority signs: the wincode-serialized payload. The leading
    /// `signer_program_id` doubles as the signing domain separator.
    pub fn signing_bytes(&self) -> wincode::WriteResult<Vec<u8>> {
        wincode::serialize(self)
    }
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
        super::{Instruction, SubmitEnvelope, SubmitPayload},
        alloc::vec,
        solana_address::Address,
        solana_signature::Signature,
    };

    fn example_envelope() -> SubmitEnvelope {
        SubmitEnvelope {
            signatures: vec![Signature::from([7; 64])],
            payload: SubmitPayload {
                signer_program_id: crate::id(),
                executor_program_id: Address::new_from_array([1; 32]),
                executor_instruction_data: vec![2, 3],
            },
        }
    }

    #[test]
    fn instruction_tags_match_wire_format() {
        assert_eq!(
            wincode::serialize(&Instruction::Submit(example_envelope())).unwrap()[0],
            0
        );
    }

    #[test]
    fn submit_round_trips() {
        let bytes = wincode::serialize(&Instruction::Submit(example_envelope())).unwrap();
        assert_eq!(
            Instruction::try_from_bytes(&bytes).unwrap(),
            Instruction::Submit(example_envelope())
        );
    }

    #[test]
    fn try_from_bytes_rejects_unknown() {
        assert!(Instruction::try_from_bytes(&[1]).is_err());
        assert!(Instruction::try_from_bytes(&[255]).is_err());
    }

    #[test]
    fn signing_bytes_are_serialized_payload() {
        let payload = example_envelope().payload;
        assert_eq!(
            payload.signing_bytes().unwrap(),
            wincode::serialize(&payload).unwrap()
        );
        // The signed bytes begin with the signer program id
        assert!(
            payload
                .signing_bytes()
                .unwrap()
                .starts_with(crate::id().as_ref())
        );
    }

    // test locking the exact signed wire layout
    #[test]
    fn submit_payload_wire_format_is_frozen() {
        let payload = SubmitPayload {
            signer_program_id: Address::new_from_array([1; 32]),
            executor_program_id: Address::new_from_array([2; 32]),
            executor_instruction_data: vec![0xAA, 0xBB],
        };

        let mut expected = vec![];
        expected.extend_from_slice(&[1u8; 32]); // signer_program_id
        expected.extend_from_slice(&[2u8; 32]); // executor_program_id
        expected.extend_from_slice(&2u64.to_le_bytes()); // Vec<u8> length: u64 little-endian
        expected.extend_from_slice(&[0xAA, 0xBB]); // executor_instruction_data

        assert_eq!(payload.signing_bytes().unwrap(), expected);
    }
}
