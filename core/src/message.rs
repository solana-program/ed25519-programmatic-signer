use {
    crate::instruction::consume_nonce,
    alloc::vec::Vec,
    core::{fmt, iter, slice},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::{
        AccountKeys, CompileError, MessageHeader,
        v1::{self, MessageError},
    },
};

/// Reasons [`execution_message`] cannot build a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionMessageError {
    /// One of the instructions could not be compiled against the message's account keys.
    Compile(CompileError),
    /// The message breaks a v1 message rule, such as a signature or address limit.
    Invalid(MessageError),
    /// No non-invoked signer is writable and every non-invoked signer is referenced by a payload
    /// instruction.
    NoWritableSigner,
}

impl fmt::Display for ExecutionMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(err) => write!(f, "failed to compile execution instructions: {err}"),
            Self::Invalid(err) => err.fmt(f),
            Self::NoWritableSigner => f.write_str(
                "every non-invoked signer is referenced read-only by instructions, leaving no \
                 writable signer for index 0",
            ),
        }
    }
}

impl core::error::Error for ExecutionMessageError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Compile(err) => Some(err),
            Self::Invalid(err) => Some(err),
            Self::NoWritableSigner => None,
        }
    }
}

impl From<CompileError> for ExecutionMessageError {
    fn from(err: CompileError) -> Self {
        Self::Compile(err)
    }
}

impl From<MessageError> for ExecutionMessageError {
    fn from(err: MessageError) -> Self {
        Self::Invalid(err)
    }
}

/// Builds the `v1::Message` that authorities sign for
/// [`Instruction::Execute`](crate::instruction::Instruction::Execute).
///
/// Places [`Instruction::ConsumeNonce`](crate::instruction::Instruction::ConsumeNonce) at
/// index 0, appends `instructions`, and sets `lifetime_specifier` to `expected_nonce`.
///
/// Pass any required signers that do not appear in the payload in `additional_signers` (for
/// example, the authority key behind a [`ProgrammaticSigner`](crate::pda::ProgrammaticSigner)).
/// They stay read-only unless a payload instruction writes to them.
///
/// The Solana v1 message format requires account 0 to be a writable signer that is not invoked as
/// a program. If no signer already qualifies, this marks an unused, non-invoked signer writable
/// for account 0, trying `nonce_authority` first and then `additional_signers`. Because no
/// payload instruction references that account, no CPI gains extra write access. Returns
/// [`ExecutionMessageError::NoWritableSigner`] if none is available.
pub fn execution_message(
    program_id: &Address,
    nonce_account: &Address,
    nonce_authority: &Address,
    expected_nonce: Hash,
    instructions: &[Instruction],
    additional_signers: &[Address],
) -> Result<v1::Message, ExecutionMessageError> {
    // `Execute` requires `ConsumeNonce` at index 0 so the signed message binds the program ID,
    // nonce account, and nonce authority
    let consume_ix = consume_nonce(program_id, nonce_account, nonce_authority);
    let mut accounts = collect_accounts(&consume_ix, instructions, additional_signers);

    // Enforce the address limit early so all header counts fit in `u8`.
    if accounts.len() > usize::from(v1::MAX_ADDRESSES) {
        return Err(MessageError::TooManyAddresses.into());
    }

    // Account 0 must be a writable signer that is not an invoked program. If none exists, promote
    // a signer not referenced by `instructions`, checking `nonce_authority` before
    // `additional_signers`.
    if !accounts
        .iter()
        .any(|a| a.is_signer && a.is_writable && !a.is_invoked_program)
    {
        let fallback = accounts
            .iter_mut()
            .find(|a| a.is_signer && !a.is_invoked_program && !a.referenced_in_payload)
            .ok_or(ExecutionMessageError::NoWritableSigner)?;
        fallback.is_writable = true;
    }

    // Sort into the 4 header groups (writable signers, read-only signers, writable unsigned,
    // read-only unsigned)
    accounts.sort_by_key(|a| {
        (
            // Signers before non-signers
            !a.is_signer,
            // Writable before read-only within each group
            !a.is_writable,
            // Non-invoked writable signers first so an invoked program never lands at index 0
            a.is_signer && a.is_writable && a.is_invoked_program,
        )
    });

    let num_required_signatures = accounts.iter().filter(|a| a.is_signer).count();
    let num_readonly_signed = accounts
        .iter()
        .filter(|a| a.is_signer && !a.is_writable)
        .count();
    let num_readonly_unsigned = accounts
        .iter()
        .filter(|a| !a.is_signer && !a.is_writable)
        .count();
    let addresses: Vec<Address> = accounts.into_iter().map(|a| a.key).collect();

    let account_keys = AccountKeys::new(&addresses, None);
    let mut compiled_instructions =
        account_keys.try_compile_instructions(slice::from_ref(&consume_ix))?;
    compiled_instructions.extend(account_keys.try_compile_instructions(instructions)?);

    let header = MessageHeader {
        num_required_signatures: u8::try_from(num_required_signatures).unwrap(),
        num_readonly_signed_accounts: u8::try_from(num_readonly_signed).unwrap(),
        num_readonly_unsigned_accounts: u8::try_from(num_readonly_unsigned).unwrap(),
    };

    let message = v1::Message::new(
        header,
        v1::TransactionConfig::default(),
        expected_nonce,
        addresses,
        compiled_instructions,
    );
    message.validate()?;

    Ok(message)
}

/// Account entry and merged flags used to build and sort `v1::Message::account_keys`.
struct MessageAccount {
    key: Address,
    is_signer: bool,
    is_writable: bool,
    /// Appears as an instruction's `program_id`, so it cannot occupy index 0.
    is_invoked_program: bool,
    /// Appears in a payload instruction's `accounts`, so it cannot be promoted to writable.
    referenced_in_payload: bool,
}

impl MessageAccount {
    // Yields each account in `ix.accounts` followed by `ix.program_id` as an invoked program
    fn from_instruction(
        ix: &Instruction,
        referenced_in_payload: bool,
    ) -> impl Iterator<Item = Self> + '_ {
        ix.accounts
            .iter()
            .map(move |meta| Self::from_meta(meta, referenced_in_payload))
            .chain(iter::once(Self::invoked_program(ix.program_id)))
    }

    fn from_meta(meta: &AccountMeta, referenced_in_payload: bool) -> Self {
        Self {
            key: meta.pubkey,
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
            is_invoked_program: false,
            referenced_in_payload,
        }
    }

    fn invoked_program(key: Address) -> Self {
        Self {
            key,
            is_signer: false,
            is_writable: false,
            is_invoked_program: true,
            referenced_in_payload: false,
        }
    }

    fn signer(key: Address) -> Self {
        Self {
            key,
            is_signer: true,
            is_writable: false,
            is_invoked_program: false,
            referenced_in_payload: false,
        }
    }

    // Keeps any flag that is true in either appearance so privileges only escalate, never downgrade
    fn merge(&mut self, other: Self) {
        self.is_signer |= other.is_signer;
        self.is_writable |= other.is_writable;
        self.is_invoked_program |= other.is_invoked_program;
        self.referenced_in_payload |= other.referenced_in_payload;
    }
}

// Deduplicates every account referenced by `consume_ix`, `payload_ixs`, and `additional_signers`
// in insertion order, merging privileges across appearances
fn collect_accounts(
    consume_ix: &Instruction,
    payload_ixs: &[Instruction],
    additional_signers: &[Address],
) -> Vec<MessageAccount> {
    let mut accounts: Vec<MessageAccount> = Vec::new();
    for account in MessageAccount::from_instruction(consume_ix, false)
        .chain(
            payload_ixs
                .iter()
                .flat_map(|ix| MessageAccount::from_instruction(ix, true)),
        )
        .chain(
            additional_signers
                .iter()
                .copied()
                .map(MessageAccount::signer),
        )
    {
        if let Some(existing) = accounts.iter_mut().find(|a| a.key == account.key) {
            existing.merge(account);
        } else {
            accounts.push(account);
        }
    }
    accounts
}
