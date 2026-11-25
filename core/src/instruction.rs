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
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite},
};

/// `WithdrawNonce` amount that withdraws the nonce account's entire balance, closing it.
///
/// Unlike the exact balance, this stays valid if the balance grows before the withdrawal lands,
/// such as from a transfer into the nonce account. Wallets render it as the raw `u64::MAX`.
pub const WITHDRAW_ALL: u64 = u64::MAX;

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

    /// Withdraws lamports from a nonce account to a destination.
    ///
    /// The stored authority must sign. If the account is not an initialized nonce,
    /// the authority must instead be the nonce account itself, signing with its
    /// own keypair; this recovers funds from an account that was created with the
    /// wrong size. A partial withdrawal must leave at least the rent-exempt
    /// minimum. Withdrawing the entire balance closes the account (removing its
    /// data and assigning it to the system program); for an initialized nonce this
    /// requires the current slot to be strictly greater than `initialize_slot` to
    /// prevent restoring the initial nonce by closing and recreating in the same slot. The destination must differ from the nonce account.
    ///
    /// An amount of [`WITHDRAW_ALL`] withdraws the entire balance at execution, so a pre-signed
    /// close still succeeds after lamports are sent to the nonce account.
    ///
    /// Accounts:
    /// - `[signer]` Authority stored in the nonce account, or the nonce account
    ///   itself if it is not initialized
    /// - `[writable]` Nonce account
    /// - `[writable]` Destination receiving the lamports
    #[cfg_attr(
        feature = "codama",
        codama(display(
            intent = "Withdraw from nonce account",
            interpolated_intent = "Withdraw ${data.lamports} lamports from nonce account \
                                   ${accounts.nonceAccount} to ${accounts.destination}"
        )),
        codama(account(
            name = "authority",
            signer,
            docs = "Authority stored in the nonce account, or the nonce account itself if it is \
                    not initialized",
            display(label = "Nonce authority")
        )),
        codama(account(
            name = "nonce_account",
            writable,
            docs = "Nonce account to withdraw from",
            display(label = "Nonce account to withdraw from")
        )),
        codama(account(
            name = "destination",
            writable,
            docs = "Account receiving the lamports",
            display(label = "Withdrawal destination")
        ))
    )]
    WithdrawNonce {
        /// Number of lamports to withdraw. Withdrawing the full balance closes the account.
        /// [`WITHDRAW_ALL`] withdraws the full balance, whatever it is at execution.
        lamports: u64,
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

/// Creates a `WithdrawNonce` instruction for `program_id`. Withdrawing the full balance, or
/// [`WITHDRAW_ALL`], closes the account.
///
/// `authority` is the stored nonce authority, or `nonce_account` itself to recover funds from
/// an account that is not initialized (for example, one created with the wrong size).
#[cfg(feature = "client")]
pub fn withdraw_nonce(
    program_id: &Address,
    authority: &Address,
    nonce_account: &Address,
    destination: &Address,
    lamports: u64,
) -> SolanaInstruction {
    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::WithdrawNonce { lamports },
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new(*destination, false),
        ],
    )
}

#[cfg(test)]
mod tests {
    use {super::Instruction, solana_program_error::ProgramError, test_case::test_case};

    #[test_case(Instruction::InitializeNonce, 0; "initialize")]
    #[test_case(Instruction::WithdrawNonce { lamports: 42 }, 1; "withdraw")]
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
