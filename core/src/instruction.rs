#[cfg(feature = "codama")]
use codama_macros::CodamaInstructions;
#[cfg(feature = "client")]
use {
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction as SolanaInstruction},
    solana_sdk_ids::sysvar::slot_hashes,
};
use {
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by an SPL Programmatic Signer program.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
#[cfg_attr(
    feature = "codama",
    derive(CodamaInstructions),
    codama(enum_discriminator(size = number(u8)))
)]
pub enum Instruction {
    /// Initializes a caller-created, program-owned nonce account for an authority.
    ///
    /// Accounts:
    /// - `[writable]` Nonce account
    /// - `[]` Authority to store in the nonce account
    /// - `[]` `SlotHashes` sysvar
    #[cfg_attr(
        feature = "codama",
        codama(display(
            intent = "Initialize nonce account",
            interpolated_intent = "Initialize nonce account ${accounts.nonceAccount} with \
                                   authority ${accounts.authority}"
        )),
        codama(account(name = "nonce_account", writable, docs = "Nonce account to initialize")),
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
    InitializeNonce,
}

impl Instruction {
    #[inline(always)]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, ProgramError> {
        wincode::deserialize_exact(bytes).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// Creates an `InitializeNonce` instruction for `program_id`.
#[cfg(feature = "client")]
pub fn initialize_nonce(
    program_id: &Address,
    nonce_account: &Address,
    authority: &Address,
) -> SolanaInstruction {
    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::InitializeNonce,
        alloc::vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
    )
}

#[cfg(test)]
mod tests {
    use {super::Instruction, solana_program_error::ProgramError, test_case::test_case};

    #[test]
    fn initialize_tag_and_round_trip() {
        let instruction = Instruction::InitializeNonce;
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(bytes, [0]);
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);

        let mut trailing = bytes;
        trailing.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            Instruction::try_from_bytes(&trailing),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test_case(1; "next tag")]
    #[test_case(u8::MAX; "max tag")]
    fn try_from_bytes_rejects_unknown(tag: u8) {
        assert_eq!(
            Instruction::try_from_bytes(&[tag]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
