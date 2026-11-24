#[cfg(feature = "codama")]
use codama_macros::CodamaInstructions;
#[cfg(feature = "client")]
use {
    alloc::vec,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction as SolanaInstruction},
    solana_sdk_ids::sysvar::slot_hashes,
};
use {
    solana_hash::Hash,
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

    /// Advances a nonce account to invalidate pending authorizations.
    ///
    /// Accounts:
    /// - `[signer]` Stored nonce authority
    /// - `[writable]` Nonce account
    #[cfg_attr(
        feature = "codama",
        codama(display(intent = "Advance nonce account")),
        codama(account(
            name = "authority",
            signer,
            docs = "Authority stored in the nonce account",
            display(label = "Nonce authority")
        )),
        codama(account(name = "nonce_account", writable, docs = "Nonce account to advance"))
    )]
    AdvanceNonce {
        /// Must match the nonce currently stored in the account so an advance
        /// cannot be replayed or accidentally skip past a newer state.
        #[cfg_attr(feature = "codama", codama(type = public_key))]
        current_nonce: Hash,
        /// Value mixed into the hash that derives the next nonce. Pass random
        /// bytes for an unpredictable next nonce or a chosen value to compute
        /// the next nonce offline.
        #[cfg_attr(feature = "codama", codama(type = public_key))]
        salt: Hash,
    },
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
        vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
    )
}

/// Creates an `AdvanceNonce` instruction for `program_id`.
#[cfg(feature = "client")]
pub fn advance_nonce(
    program_id: &Address,
    nonce_account: &Address,
    authority: &Address,
    current_nonce: Hash,
    salt: Hash,
) -> SolanaInstruction {
    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::AdvanceNonce {
            current_nonce,
            salt,
        },
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*nonce_account, false),
        ],
    )
}

#[cfg(test)]
mod tests {
    use {
        super::Instruction, solana_hash::Hash, solana_program_error::ProgramError,
        test_case::test_case,
    };

    #[test_case(Instruction::InitializeNonce, 0; "initialize")]
    #[test_case(
        Instruction::AdvanceNonce {
            current_nonce: Hash::new_from_array([1; 32]),
            salt: Hash::new_from_array([2; 32]),
        },
        1;
        "advance"
    )]
    fn instruction_tags_and_round_trip(instruction: Instruction, expected_tag: u8) {
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(bytes[0], expected_tag);
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);

        let mut trailing = bytes;
        trailing.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            Instruction::try_from_bytes(&trailing),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test_case(2; "next tag")]
    #[test_case(u8::MAX; "max tag")]
    fn try_from_bytes_rejects_unknown(tag: u8) {
        assert_eq!(
            Instruction::try_from_bytes(&[tag]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
