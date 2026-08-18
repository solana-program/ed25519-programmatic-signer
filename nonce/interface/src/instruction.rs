#[cfg(feature = "codama")]
use codama_macros::CodamaInstructions;
use {
    solana_hash::Hash,
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Nonce program.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
#[cfg_attr(
    feature = "codama",
    derive(CodamaInstructions),
    codama(enum_discriminator(size = number(u8)))
)]
pub enum Instruction {
    /// Initializes a nonce account for an authority.
    ///
    /// The caller must first create and fund the account. Recommended to include
    /// the system `CreateAccount` instruction and `Initialize` in the same transaction so no
    /// other transaction can initialize the account first.
    ///
    /// On success, the program:
    /// 1. Verifies the account is uninitialized, rent-exempt, and owned by this program.
    /// 2. Derives the initial `nonce` by hashing the initialization tag, nonce account
    ///    address, program id, and latest slot hash.
    /// 3. Writes `Nonce { nonce, authority }` into the account data.
    ///
    /// Instruction data is the discriminator only.
    ///
    /// Required accounts.
    /// - `[writable]` Nonce account
    /// - `[]` Authority to store in the nonce account
    /// - `[]` `SlotHashes` sysvar
    #[cfg_attr(
        feature = "codama",
        codama(display(
            intent = "Initialize nonce account",
            interpolated_intent = "Initialize nonce account ${accounts.nonceAccount} with authority ${accounts.authority}"
        )),
        codama(account(
            name = "nonce_account",
            writable,
            docs = "Nonce account to initialize"
        )),
        codama(account(
            name = "authority",
            docs = "Authority stored in the nonce account",
            display(label = "Nonce authority")
        )),
        codama(account(
            name = "slot_hashes",
            docs = "Slot Hashes sysvar",
            default_value = sysvar("slot_hashes"),
            display(skip = always)
        ))
    )]
    Initialize,

    /// Consumes the stored nonce and advances it to a fresh value.
    ///
    /// Consumers verify the stored nonce by reading the account, then invoke this instruction
    /// via CPI after their work succeeds. `current_nonce` is re-checked here so the
    /// nonce cannot be consumed twice within one transaction.
    ///
    /// By convention, `transition_commitment` is a hash of whatever action the advancement
    /// authorizes. The program does not validate it or mix in entropy of its own, so an authority
    /// that fixes its commitments in advance knows every future nonce value. This allows signing
    /// an ordered batch of transactions up front, where each becomes valid only after its
    /// predecessor executes. Advancing with a different commitment at any step yields a different
    /// successor and invalidates everything signed against the abandoned branch.
    ///
    /// Instruction data is the discriminator followed by a serialized [`AdvanceNonceArgs`].
    ///
    /// On success, the program:
    /// 1. Verifies the stored authority matches the authority account, which must carry
    ///    runtime signer privilege.
    /// 2. Verifies the stored nonce equals `current_nonce`.
    /// 3. Stores the next nonce by hashing the advancement tag, program id, nonce account
    ///    address, old nonce, and transition commitment.
    ///
    /// Required accounts.
    /// - `[signer]` Authority stored in the nonce account
    /// - `[writable]` Nonce account
    #[cfg_attr(
        feature = "codama",
        codama(display(
            intent = "Advance nonce",
            interpolated_intent = "Advance nonce account ${accounts.nonceAccount} by consuming current nonce ${data.currentNonce}"
        )),
        codama(account(
            name = "authority",
            signer,
            docs = "Authority stored in the nonce account",
            display(label = "Nonce authority")
        )),
        codama(account(
            name = "nonce_account",
            writable,
            docs = "Nonce account to advance"
        ))
    )]
    Advance(
        #[cfg_attr(
            feature = "codama",
            codama(name = "current_nonce"),
            codama(type = public_key)
        )]
        AdvanceNonceArgs,
    ),

    /// Closes a nonce account. Not yet implemented.
    #[cfg_attr(feature = "codama", codama(skip))]
    Close,
}

/// Payload for nonce advancement.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct AdvanceNonceArgs {
    /// Nonce value the account must currently store.
    pub current_nonce: Hash,
    /// Value the successor nonce commits to, conventionally a hash of the action being
    /// authorized. See [`Instruction::Advance`].
    pub transition_commitment: Hash,
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
        super::{AdvanceNonceArgs, Hash, Instruction},
        solana_program_error::ProgramError,
        test_case::test_case,
    };

    const ADVANCE_IX: Instruction = Instruction::Advance(AdvanceNonceArgs {
        current_nonce: Hash::new_from_array([1; 32]),
        transition_commitment: Hash::new_from_array([2; 32]),
    });

    #[test_case(Instruction::Initialize, 0)]
    #[test_case(ADVANCE_IX, 1)]
    #[test_case(Instruction::Close, 2)]
    fn instruction_tag_matches_wire_format(instruction: Instruction, expected: u8) {
        assert_eq!(wincode::serialize(&instruction).unwrap()[0], expected);
    }

    #[test_case(Instruction::Initialize)]
    #[test_case(ADVANCE_IX)]
    #[test_case(Instruction::Close)]
    fn instruction_round_trips(instruction: Instruction) {
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);
    }

    #[test_case(Instruction::Initialize)]
    #[test_case(ADVANCE_IX)]
    #[test_case(Instruction::Close)]
    fn instruction_rejects_trailing_data(instruction: Instruction) {
        let mut bytes = wincode::serialize(&instruction).unwrap();
        bytes.extend_from_slice(&[1, 2, 3]);

        assert_eq!(
            Instruction::try_from_bytes(&bytes),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test_case(3; "next tag")]
    #[test_case(u8::MAX; "maximum tag")]
    fn try_from_bytes_rejects_unknown(tag: u8) {
        assert_eq!(
            Instruction::try_from_bytes(&[tag]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
