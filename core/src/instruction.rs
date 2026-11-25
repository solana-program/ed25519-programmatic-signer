#[cfg(feature = "codama")]
use codama_macros::CodamaInstructions;
use {
    alloc::vec::Vec,
    solana_hash::Hash,
    solana_message::{MessageHeader, compiled_instruction::CompiledInstruction},
    solana_program_error::ProgramError,
    wincode::{SchemaRead, SchemaWrite, config::DefaultConfig, containers},
};
#[cfg(feature = "client")]
use {
    alloc::{collections::BTreeSet, vec},
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction as SolanaInstruction},
    solana_message::v1,
    solana_sdk_ids::sysvar::slot_hashes,
};

/// Instructions supported by an SPL Programmatic Signer program.
///
/// Generic over `Sig`, the signature entry supplied for each required signer in
/// [`Execute`](Self::Execute) (for example, an offchain Ed25519 signature).
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
#[wincode(tag_encoding = "u8")]
#[cfg_attr(
    feature = "codama",
    derive(CodamaInstructions),
    codama(enum_discriminator(size = number(u8)))
)]
pub enum Instruction<Sig> {
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

    /// Executes instructions signed offchain on behalf of one or more
    /// [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner) accounts.
    ///
    /// A [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner) is a PDA derived from a signing
    /// key that holds assets or owns accounts onchain. Signers approve a `v1::Message` offline
    /// (for example, from cold storage or across multiple days) without being bound by blockhash
    /// expiration. Later, anyone can submit this instruction in a normal transaction and pay the
    /// network fee. The offline signers never have to sign the outer transaction.
    ///
    /// Onchain, this instruction:
    /// 1. Rebuilds the signed `v1::Message` from the instruction accounts (so public keys are not
    ///    duplicated in instruction data).
    /// 2. Verifies each signature against that message.
    /// 3. Consumes and advances the nonce account in `instructions[0]` so the message cannot be
    ///    replayed.
    /// 4. Invokes `instructions[1..]` via CPI, signing as each verified
    ///    [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner) PDA (plus any outer transaction
    ///    signers authorized by the signing scheme).
    ///
    /// Message format: `instructions[0]` must be [`ConsumeNonce`](Self::ConsumeNonce) targeting
    /// this program with the nonce account and its authority. Because `expected_nonce` is just a
    /// 32-byte hash, this binds the program ID, nonce account, and nonce authority into the signed
    /// bytes. Omit payload instructions to advance the nonce and cancel pending messages.
    ///
    /// Accounts:
    /// - Every account in `v1::Message::account_keys`, in header order.
    #[cfg_attr(
        feature = "codama",
        codama(display(intent = "Execute pre-signed instructions"))
    )]
    Execute {
        #[wincode(with = "containers::Vec<_, u8>")]
        #[cfg_attr(
            feature = "codama",
            codama(type = bytes),
            codama(display(label = "Authority signatures"))
        )]
        signatures: Vec<Sig>,
        #[cfg_attr(
            feature = "codama",
            codama(type = struct(
                field("num_required_signatures", number(u8)),
                field("num_readonly_signed_accounts", number(u8)),
                field("num_readonly_unsigned_accounts", number(u8)),
            )),
            codama(display(label = "Message header"))
        )]
        header: MessageHeader,
        #[cfg_attr(
            feature = "codama",
            codama(type = public_key),
            codama(display(label = "Expected nonce"))
        )]
        expected_nonce: Hash,
        #[wincode(with = "containers::Vec<CompiledInstruction, u8>")]
        #[cfg_attr(
            feature = "codama",
            codama(type = array(
                struct(
                    field("program_id_index", number(u8)),
                    field("accounts", array(number(u8), prefixed_count(number(short_u16)))),
                    field("data", size_prefix(bytes, number(short_u16))),
                ),
                prefixed_count(number(u8))
            )),
            codama(display(label = "Compiled instructions"))
        )]
        instructions: Vec<CompiledInstruction>,
    },

    /// Non-callable marker instruction placed at `instructions[0]` inside a signed `v1::Message`
    /// to bind the program ID, nonce account, and nonce authority. Invoking this instruction
    /// directly onchain always returns [`ConsumeNonceNotCallable`](crate::error::Error::ConsumeNonceNotCallable).
    ///
    /// Accounts:
    /// - `[writable]` Nonce account
    /// - `[signer]` Nonce authority
    #[cfg_attr(
        feature = "codama",
        codama(display(intent = "Consume nonce (marker instruction for signed messages)")),
        codama(account(name = "nonce_account", writable, docs = "Nonce account to consume")),
        codama(account(
            name = "authority",
            signer,
            docs = "Authority stored in the nonce account",
            display(label = "Nonce authority")
        ))
    )]
    ConsumeNonce,
}

impl<Sig> Instruction<Sig> {
    #[inline(always)]
    pub fn try_from_bytes<'de>(bytes: &'de [u8]) -> Result<Self, ProgramError>
    where
        Sig: SchemaRead<'de, DefaultConfig, Dst = Sig>,
    {
        wincode::deserialize_exact(bytes).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// Returns whether self-invocation instruction data is allowed inside an `Execute` payload.
///
/// Permits only nonce management instructions and blocks everything else (including empty data,
/// nested `Execute`, nested `ConsumeNonce`, and unknown discriminators).
#[inline]
pub fn is_allowed_self_instruction(data: &[u8]) -> bool {
    matches!(
        Instruction::<()>::try_from_bytes(data),
        Ok(Instruction::InitializeNonce)
    )
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
        &Instruction::<()>::InitializeNonce,
        vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*authority, false),
            AccountMeta::new_readonly(slot_hashes::id(), false),
        ],
    )
}

/// Creates a `ConsumeNonce` marker instruction for `program_id` placed at `instructions[0]`
/// inside a signed `v1::Message`.
#[cfg(feature = "client")]
pub fn consume_nonce(
    program_id: &Address,
    nonce_account: &Address,
    nonce_authority: &Address,
) -> SolanaInstruction {
    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::<()>::ConsumeNonce,
        vec![
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new_readonly(*nonce_authority, true),
        ],
    )
}

/// Builds the onchain `Execute` instruction for `program_id` from the signed `v1::Message` and
/// one `Sig` entry per required signer.
///
/// The message's `account_keys` are supplied as instruction accounts rather than repeated inside
/// instruction data. All returned accounts start with `is_signer = false`. Callers mark any outer
/// relay signers as `is_signer = true`.
#[cfg(feature = "client")]
pub fn execute<Sig>(
    program_id: &Address,
    signatures: Vec<Sig>,
    message: &v1::Message,
) -> SolanaInstruction
where
    Sig: SchemaWrite<DefaultConfig, Src = Sig>,
{
    let accounts = message
        .account_keys
        .iter()
        .enumerate()
        .map(|(index, key)| AccountMeta {
            pubkey: *key,
            is_signer: false,
            is_writable: message
                .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
        })
        .collect();

    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::Execute {
            signatures,
            header: message.header,
            expected_nonce: message.lifetime_specifier,
            instructions: message.instructions.clone(),
        },
        accounts,
    )
}

#[cfg(test)]
mod tests {
    use {
        super::{Instruction, is_allowed_self_instruction},
        alloc::{vec, vec::Vec},
        solana_hash::Hash,
        solana_message::{MessageHeader, compiled_instruction::CompiledInstruction},
        solana_program_error::ProgramError,
        test_case::test_case,
    };

    fn sample_execute() -> Instruction<u8> {
        Instruction::Execute {
            signatures: vec![1, 2, 3],
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            expected_nonce: Hash::new_from_array([9; 32]),
            instructions: vec![CompiledInstruction {
                program_id_index: 3,
                accounts: vec![0, 1],
                data: vec![2],
            }],
        }
    }

    #[test_case(Instruction::InitializeNonce, 0; "initialize")]
    #[test_case(sample_execute(), 1; "execute")]
    #[test_case(Instruction::ConsumeNonce, 2; "consume")]
    fn instruction_tags_and_round_trip(instruction: Instruction<u8>, expected_tag: u8) {
        let bytes = wincode::serialize(&instruction).unwrap();
        assert_eq!(bytes[0], expected_tag);
        assert_eq!(Instruction::try_from_bytes(&bytes).unwrap(), instruction);

        let mut trailing = bytes;
        trailing.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            Instruction::<u8>::try_from_bytes(&trailing),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test_case(3; "next tag")]
    #[test_case(u8::MAX; "max tag")]
    fn try_from_bytes_rejects_unknown(tag: u8) {
        assert_eq!(
            Instruction::<u8>::try_from_bytes(&[tag]),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test_case(
        wincode::serialize(&Instruction::<()>::InitializeNonce).unwrap(),
        true;
        "initialize"
    )]
    #[test_case(wincode::serialize(&sample_execute()).unwrap(), false; "execute")]
    #[test_case(
        wincode::serialize(&Instruction::<()>::ConsumeNonce).unwrap(),
        false;
        "consume"
    )]
    #[test_case(vec![], false; "empty")]
    #[test_case(vec![3], false; "unknown")]
    fn self_instruction_filter_allows_only_nonce_management(data: Vec<u8>, allowed: bool) {
        assert_eq!(is_allowed_self_instruction(&data), allowed);
    }
}
