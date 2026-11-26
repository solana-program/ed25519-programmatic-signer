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

/// Index of a required signer in `message.account_keys[..num_required_signatures]`.
pub type AccountIndex = u8;

/// An offchain signature from an authority over the execution message in [`Instruction::Execute`].
///
/// Authorizes the authority's [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner) PDA (or acts
/// as an offchain approval on the authority's signer slot) without forwarding a relay transaction
/// signature on the authority itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct AuthoritySignature<Sig> {
    /// Index of the signing authority in `message.account_keys[..num_required_signatures]`.
    pub account_index: AccountIndex,
    /// Raw offchain signature over the serialized `v1::Message`.
    pub signature: Sig,
}

/// Instructions supported by an SPL Programmatic Signer program.
///
/// Generic over `Sig`, the raw offchain signature type verified by the program's signing scheme
/// (for example, a 64-byte Ed25519 signature).
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
    /// To close the account without stating its exact balance, use `CloseNonce`.
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
        lamports: u64,
    },

    /// Withdraws the entire balance of a nonce account to a destination, closing it.
    ///
    /// Behaves like `WithdrawNonce` for the full balance, except the balance is read at
    /// execution. A pre-signed close therefore still succeeds after lamports are sent to the
    /// nonce account. The same authority, same-slot close, and destination rules apply.
    ///
    /// Accounts:
    /// - `[signer]` Authority stored in the nonce account, or the nonce account
    ///   itself if it is not initialized
    /// - `[writable]` Nonce account
    /// - `[writable]` Destination receiving the lamports
    #[cfg_attr(
        feature = "codama",
        codama(display(
            intent = "Close nonce account",
            interpolated_intent = "Close nonce account ${accounts.nonceAccount} and send its \
                                   balance to ${accounts.destination}"
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
            docs = "Nonce account to close",
            display(label = "Nonce account to close")
        )),
        codama(account(
            name = "destination",
            writable,
            docs = "Account receiving the lamports",
            display(label = "Withdrawal destination")
        ))
    )]
    CloseNonce,

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
    ///    [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner) PDA (plus any listed
    ///    `relay_signers`).
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
        #[wincode(with = "containers::Vec<AuthoritySignature<Sig>, u8>")]
        #[cfg_attr(
            feature = "codama",
            codama(type = bytes),
            codama(display(label = "Authority signatures"))
        )]
        signatures: Vec<AuthoritySignature<Sig>>,
        #[wincode(with = "containers::Vec<AccountIndex, u8>")]
        #[cfg_attr(
            feature = "codama",
            codama(type = array(number(u8), prefixed_count(number(u8)))),
            codama(display(label = "Relay signers"))
        )]
        relay_signers: Vec<AccountIndex>,
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

/// Creates a `WithdrawNonce` instruction for `program_id`. Withdrawing the full balance closes
/// the account.
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
        &Instruction::<()>::WithdrawNonce { lamports },
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new(*destination, false),
        ],
    )
}

/// Creates a `CloseNonce` instruction for `program_id`, which withdraws the full balance at
/// execution.
///
/// `authority` is the stored nonce authority, or `nonce_account` itself to recover funds from
/// an account that is not initialized (for example, one created with the wrong size).
#[cfg(feature = "client")]
pub fn close_nonce(
    program_id: &Address,
    authority: &Address,
    nonce_account: &Address,
    destination: &Address,
) -> SolanaInstruction {
    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::<()>::CloseNonce,
        vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*nonce_account, false),
            AccountMeta::new(*destination, false),
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

/// Builds an [`Instruction::Execute`] instruction for `program_id` from a signed `v1::Message`,
/// offchain [`AuthoritySignature`] entries, and explicit `relay_signers`.
///
/// Passes `message.account_keys` as the instruction accounts so public keys are not duplicated in
/// instruction data. Accounts that are neither the nonce account nor used by any payload
/// instruction stay read-only to avoid unnecessary write locks on the outer transaction.
///
/// Accounts whose index appears in `relay_signers` are marked `is_signer = true`, and all other
/// accounts start with `is_signer = false`.
#[cfg(feature = "client")]
pub fn execute<Sig>(
    program_id: &Address,
    signatures: Vec<AuthoritySignature<Sig>>,
    relay_signers: Vec<AccountIndex>,
    message: &v1::Message,
) -> SolanaInstruction
where
    Sig: SchemaWrite<DefaultConfig, Src = Sig>,
{
    // `instructions[0]` is `ConsumeNonce`, whose first account is the nonce account
    let nonce_account = message
        .instructions
        .first()
        .and_then(|ix| ix.accounts.first())
        .map(|&idx| usize::from(idx));
    let payload_ixs = message.instructions.get(1..).unwrap_or_default();

    // Mirror `message.account_keys`, skipping write locks on accounts not used onchain (such as
    // an unused signer promoted to index 0 to satisfy v1 message rules)
    let accounts = message
        .account_keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let used_by_nonce_or_payload = nonce_account == Some(index)
                || payload_ixs
                    .iter()
                    .any(|ix| ix.accounts.iter().any(|&idx| usize::from(idx) == index));
            AccountMeta {
                pubkey: *key,
                is_signer: relay_signers.iter().any(|&idx| usize::from(idx) == index),
                is_writable: used_by_nonce_or_payload
                    && message
                        .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
            }
        })
        .collect();

    SolanaInstruction::new_with_wincode(
        *program_id,
        &Instruction::Execute {
            signatures,
            relay_signers,
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
        super::{AuthoritySignature, Instruction},
        alloc::vec,
        solana_hash::Hash,
        solana_message::{MessageHeader, compiled_instruction::CompiledInstruction},
        solana_program_error::ProgramError,
        test_case::test_case,
    };

    fn sample_execute() -> Instruction<u8> {
        Instruction::Execute {
            signatures: vec![
                AuthoritySignature {
                    account_index: 1,
                    signature: 7,
                },
                AuthoritySignature {
                    account_index: 2,
                    signature: 9,
                },
            ],
            relay_signers: vec![2],
            header: MessageHeader {
                num_required_signatures: 3,
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

    #[test]
    fn authority_signature_round_trip() {
        let sig = AuthoritySignature {
            account_index: 2,
            signature: 7u8,
        };
        let bytes = wincode::serialize(&sig).unwrap();
        assert_eq!(bytes, vec![2, 7]);
        assert_eq!(
            wincode::deserialize_exact::<AuthoritySignature<u8>>(&bytes).unwrap(),
            sig
        );
    }

    #[test_case(Instruction::InitializeNonce, 0; "initialize")]
    #[test_case(Instruction::WithdrawNonce { lamports: 42 }, 1; "withdraw")]
    #[test_case(Instruction::CloseNonce, 2; "close")]
    #[test_case(sample_execute(), 3; "execute")]
    #[test_case(Instruction::ConsumeNonce, 4; "consume")]
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

    #[test_case(5; "next tag")]
    #[test_case(u8::MAX; "max tag")]
    fn try_from_bytes_rejects_unknown(tag: u8) {
        assert_eq!(
            Instruction::<u8>::try_from_bytes(&[tag]),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
