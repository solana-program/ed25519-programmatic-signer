//! Builder for the authorization message signed by authorities.

use {
    alloc::vec::Vec,
    core::{fmt, iter},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::{
        AccountKeys, CompileError, MessageHeader,
        v1::{self, MessageError},
    },
};

/// Reasons [`authorization_message`] cannot build a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorizationMessageError {
    /// The executor instruction could not be compiled against the message's account keys.
    Compile(CompileError),
    /// The message breaks a v1 message rule, such as a signature or address limit.
    Invalid(MessageError),
}

impl fmt::Display for AuthorizationMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(err) => write!(f, "failed to compile executor instruction: {err}"),
            Self::Invalid(err) => err.fmt(f),
        }
    }
}

impl core::error::Error for AuthorizationMessageError {}

impl From<CompileError> for AuthorizationMessageError {
    fn from(err: CompileError) -> Self {
        Self::Compile(err)
    }
}

impl From<MessageError> for AuthorizationMessageError {
    fn from(err: MessageError) -> Self {
        Self::Invalid(err)
    }
}

/// Builds the v1 authorization message that the authorities sign over the executor instruction.
///
/// The returned message is validated against the v1 limits. Inputs beyond `v1::MAX_SIGNATURES`
/// authorities or `v1::MAX_ADDRESSES` account keys, duplicate authorities, and an empty authority
/// list return [`AuthorizationMessageError::Invalid`] with the [`MessageError`] naming the violated
/// rule.
///
/// ```text
/// account_keys                    privileges
/// ┌──────────────────────────────┬─────────────────┐
/// │ authorities the executor     │ writable signer │
/// │ writes to (at least 1)       │                 │
/// ├──────────────────────────────┼─────────────────┤
/// │ remaining authorities        │ readonly signer │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor writable accounts   │ writable        │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor program id          │ readonly        │
/// ├──────────────────────────────┼─────────────────┤
/// │ executor readonly accounts   │ readonly        │
/// └──────────────────────────────┴─────────────────┘
/// ```
///
/// The executor instruction's original `AccountMeta::is_signer` flags do not grant signer
/// privilege. CPI signer privilege comes only from required relay transaction signers and from
/// `ProgrammaticSigner` PDA promotion.
pub fn authorization_message(
    executor_instruction: &Instruction,
    authorities: &[Address],
) -> Result<v1::Message, AuthorizationMessageError> {
    // Checked first so the header counts below fit in a u8.
    if authorities.len() > usize::from(v1::MAX_SIGNATURES) {
        return Err(MessageError::TooManySignatures.into());
    }

    // Authorities the executor writes to must land in the header's writable-signer range, so
    // they are ordered ahead of the readonly authorities.
    let (writable_authorities, readonly_authorities): (Vec<_>, Vec<_>) =
        authorities.iter().copied().partition(|authority| {
            executor_instruction
                .accounts
                .iter()
                .any(|meta| meta.is_writable && meta.pubkey == *authority)
        });

    // Every message version requires at least one writable signer, the fee payer.
    // An authorization message pays no fees, so when the executor writes to no authority the first
    // one carries the writable flag anyway. The over-grant is benign. The flag does not grant
    // CPI signer privilege and a writable account without signer privilege accepts nothing
    // beyond lamport credits.
    let writable_signers_count = writable_authorities.len().max(1);

    let mut writable_unsigned = Vec::new();
    let mut readonly_unsigned = Vec::new();

    // Authorities and the executor program id already have dedicated slots, so only the
    // remaining executor accounts need unsigned slots.
    for meta in &executor_instruction.accounts {
        if authorities.contains(&meta.pubkey) {
            continue;
        }
        if meta.pubkey == executor_instruction.program_id {
            continue;
        }
        if meta.is_writable {
            if !writable_unsigned.contains(&meta.pubkey) {
                writable_unsigned.push(meta.pubkey);
            }
        } else if !readonly_unsigned.contains(&meta.pubkey) {
            readonly_unsigned.push(meta.pubkey);
        }
    }

    // A writable occurrence wins over any readonly occurrences.
    readonly_unsigned.retain(|key| !writable_unsigned.contains(key));

    // The readonly unsigned range is a suffix, so the invoked executor program id sits ahead
    // of the readonly accounts and is counted with them.
    let account_keys = [writable_authorities, readonly_authorities]
        .concat()
        .iter()
        .copied()
        .chain(writable_unsigned.iter().copied())
        .chain(iter::once(executor_instruction.program_id))
        .chain(readonly_unsigned.iter().copied())
        .collect::<Vec<_>>();

    // Checked before compiling, which would otherwise report an index overflow above 256 keys,
    // and so the header counts below fit in a u8.
    if account_keys.len() > usize::from(v1::MAX_ADDRESSES) {
        return Err(MessageError::TooManyAddresses.into());
    }

    // Compiling rewrites the program id and account pubkeys as u8 indexes into `account_keys`
    let compiled_instructions = AccountKeys::new(&account_keys, None)
        .try_compile_instructions(core::slice::from_ref(executor_instruction))?;

    let header = MessageHeader {
        num_required_signatures: authorities.len() as u8,
        num_readonly_signed_accounts: authorities.len().saturating_sub(writable_signers_count)
            as u8,
        num_readonly_unsigned_accounts: readonly_unsigned.len().saturating_add(1) as u8,
    };

    // The authorization message is never executed as a transaction, so it carries no transaction
    // config and no lifetime.
    let message = v1::Message::new(
        header,
        v1::TransactionConfig::default(),
        Hash::default(),
        account_keys,
        compiled_instructions,
    );
    message.validate()?;

    Ok(message)
}
