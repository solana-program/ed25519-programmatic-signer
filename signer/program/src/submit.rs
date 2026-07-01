//! Verifies authority signatures over a wrapped transaction, then invokes its executor instruction
//! while signing for explicitly authorized programmatic signers.

use {
    alloc::vec::Vec,
    brine_ed25519::hasher::Sha512,
    pinocchio::{
        AccountView, Address, ProgramResult,
        cpi::{Seed, Signer, invoke_signed_with_slice},
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_message::{VersionedMessage, compiled_instruction::CompiledInstruction},
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_interface::{error::Error, pda::ProgrammaticSigner},
};

/// A verified authority whose `ProgrammaticSigner` is referenced by the executor instruction.
struct AuthorizedSigner {
    authority: Address,
    programmatic_signer_index: usize,
    bump_seed: [u8; 1],
}

/// Processes the wrapped transaction and executes its single executor instruction.
pub fn process_submit(
    program_id: &Address,
    accounts: &[AccountView],
    transaction: VersionedTransaction,
) -> ProgramResult {
    let message = &transaction.message;

    // In a Solana message, the leading required signer keys are the signing authorities.
    // At least one authority must sign
    let required_signatures = usize::from(message.header().num_required_signatures);
    if required_signatures == 0 {
        return Err(Error::NoSignatures.into());
    }

    // Sanitize signature counts and message invariants
    transaction
        .sanitize()
        .map_err(|_| Error::InvalidWrappedTransaction)?;

    // Exactly one executor instruction is expected
    let [executor_instruction] = message.instructions() else {
        return Err(Error::InvalidExecutorInstructionCount.into());
    };

    // Executor account indexes must resolve to this signed static account key list.
    // The relayer must supply the same accounts in the same outer `Submit` order.
    validate_accounts_match(accounts, message)?;

    // Verify authorities signed the wrapped message
    let authorities = verify_authority_signatures(&transaction)?;

    // Only ProgrammaticSigner PDAs derived from signed authorities can be promoted
    let authorized_signers =
        collect_authorized_signers(program_id, message, executor_instruction, authorities)?;

    invoke_executor_instruction(accounts, message, executor_instruction, &authorized_signers)
}

/// Validates the outer `Submit` accounts match the wrapped message's account key list.
fn validate_accounts_match(
    outer_accounts: &[AccountView],
    wrapped_message: &VersionedMessage,
) -> ProgramResult {
    let wrapped_account_keys = wrapped_message.static_account_keys();

    if outer_accounts.len() < wrapped_account_keys.len() {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    if outer_accounts.len() > wrapped_account_keys.len() {
        return Err(Error::AccountKeyMismatch.into());
    }

    for (index, wrapped_key) in wrapped_account_keys.iter().enumerate() {
        let outer_account = &outer_accounts[index];

        if outer_account.address() != wrapped_key {
            return Err(Error::AccountKeyMismatch.into());
        }
    }

    Ok(())
}

fn verify_authority_signatures(
    transaction: &VersionedTransaction,
) -> Result<&[Address], ProgramError> {
    let required_signatures = usize::from(transaction.message.header().num_required_signatures);

    // Required signers occupy the leading account key slots. Signatures use the same indexes.
    // Infallible: sanitization guarantees a static key and a signature for every required signer.
    let authorities = transaction
        .message
        .static_account_keys()
        .get(..required_signatures)
        .unwrap();

    let signatures = transaction.signatures.get(..required_signatures).unwrap();

    let message_bytes = transaction.message.serialize();

    // Verify each authority signed the wrapped transaction message
    for (authority, signature) in authorities.iter().zip(signatures) {
        brine_ed25519::verify::<Sha512>(
            authority,
            signature.as_array(),
            &[message_bytes.as_slice()],
        )
        .map_err(|_| Error::InvalidSignature)?;
    }

    Ok(authorities)
}

fn collect_authorized_signers(
    program_id: &Address,
    message: &VersionedMessage,
    executor_instruction: &CompiledInstruction,
    authorities: &[Address],
) -> Result<Vec<AuthorizedSigner>, ProgramError> {
    let account_keys = message.static_account_keys();
    let mut authorized = Vec::<AuthorizedSigner>::new();

    // Cold authorities sign as normal Ed25519 keys. PDA signer promotion is allowed only for a
    // matching ProgrammaticSigner PDA that appears in the executor's signed account index list.
    for authority in authorities {
        let (programmatic_signer, bump) =
            ProgrammaticSigner::derive_address_and_bump(program_id, authority);

        for account_index in &executor_instruction.accounts {
            let account_index = usize::from(*account_index);

            // V0 address table lookups are never resolved, so every executor account index must
            // hit the static account keys
            let account_key = account_keys
                .get(account_index)
                .ok_or(Error::InvalidExecutorAccountIndex)?;

            // Promote the signed authority's PDA referenced by the executor
            if *account_key == programmatic_signer {
                authorized.push(AuthorizedSigner {
                    authority: *authority,
                    programmatic_signer_index: account_index,
                    bump_seed: [bump],
                });
            }
        }
    }

    Ok(authorized)
}

fn invoke_executor_instruction(
    accounts: &[AccountView],
    message: &VersionedMessage,
    executor_instruction: &CompiledInstruction,
    authorized_signers: &[AuthorizedSigner],
) -> ProgramResult {
    // The PDA seeds that authorize `invoke_signed` to sign as each programmatic signer.
    let signer_seeds: Vec<[Seed; 3]> = authorized_signers
        .iter()
        .map(|signer| {
            [
                Seed::from(ProgrammaticSigner::SEED_PREFIX),
                Seed::from(signer.authority.as_ref()),
                Seed::from(&signer.bump_seed),
            ]
        })
        .collect();
    let cpi_signers: Vec<Signer> = signer_seeds.iter().map(Signer::from).collect();

    // The executor program is selected by the signed message, not by a separate `Submit` account.
    // Infallible: sanitization guarantees the program id index hits the static account keys.
    let executor_program_id = message
        .static_account_keys()
        .get(usize::from(executor_instruction.program_id_index))
        .unwrap();

    // The CPI receives only the accounts named by the executor instruction, in signed index order.
    let mut instruction_accounts = Vec::with_capacity(executor_instruction.accounts.len());
    let mut account_views = Vec::with_capacity(executor_instruction.accounts.len());

    for account_index in &executor_instruction.accounts {
        let account_index = usize::from(*account_index);

        // Infallible: `collect_authorized_signers` bounds-checked every executor account index
        // and `validate_accounts_match` proved outer accounts mirror the static keys one-to-one.
        let account = accounts.get(account_index).unwrap();

        let is_promoted = authorized_signers
            .iter()
            .any(|signer| signer.programmatic_signer_index == account_index);

        // Real outer signers, such as a relayer co-signer, can be forwarded to the executor
        let is_forwarded_outer_signer = message.is_signer(account_index) && account.is_signer();

        // CPI privileges come from the wrapped message plus authorized PDA promotion.
        // Outer over-grants are not forwarded. Under-grants fail runtime privilege checks.
        instruction_accounts.push(InstructionAccount::new(
            account.address(),
            is_message_account_writable(account_index, message),
            is_promoted || is_forwarded_outer_signer,
        ));
        account_views.push(account);
    }

    let view = InstructionView {
        program_id: executor_program_id,
        accounts: &instruction_accounts,
        data: &executor_instruction.data,
    };
    invoke_signed_with_slice::<&AccountView>(&view, &account_views, &cpi_signers)?;

    Ok(())
}

// TODO: Replace when no-std version of: https://github.com/anza-xyz/solana-sdk/blob/042f3451979cc8e31a45a09a5627a387ac12a067/message/src/lib.rs#L155-L235
fn is_message_account_writable(index: usize, message: &VersionedMessage) -> bool {
    // [writable signers | readonly signers | writable unsigned | readonly unsigned]
    let header = message.header();
    let account_keys = message.static_account_keys();
    let required_signatures = usize::from(header.num_required_signatures);
    let writable_signers_end =
        required_signatures.saturating_sub(usize::from(header.num_readonly_signed_accounts));
    let writable_unsigned_end = account_keys
        .len()
        .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
    let is_writable_index = index < writable_signers_end
        || (required_signatures..writable_unsigned_end).contains(&index);

    is_writable_index
        && (!message.is_invoked(index)
            || account_keys.contains(&solana_sdk_ids::bpf_loader_upgradeable::id()))
}
