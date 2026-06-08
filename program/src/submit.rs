#[cfg(target_os = "solana")]
use solana_instruction::{TRANSACTION_LEVEL_STACK_HEIGHT, syscalls::get_stack_height};
use {
    crate::verifier::{SchemeState, SigningScheme, VerifiedSigner},
    alloc::vec::Vec,
    pinocchio::{
        AccountView, Address, ProgramResult,
        cpi::{Seed, Signer, invoke_signed_with_bounds},
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
        sysvars::{instructions::Instructions, slot_hashes::SlotHashes},
    },
    solana_sdk_ids::{
        bpf_loader_upgradeable,
        sysvar::{instructions as instructions_sysvar_id, slot_hashes as slot_hashes_sysvar_id},
    },
    solana_transaction::{CompiledInstruction, VersionedMessage},
    spl_ed25519_durable_signer_interface::{error::DurableSignerError, pda::DurableSignerPda},
    wincode::{SchemaRead, SchemaWrite, config::DefaultConfig},
};

/// Domain-separation tag for the nonce derivation.
const NONCE_DERIVATION_TAG: &[u8] = b"spl-ed25519-durable-signer::v1";

const MAX_CPI_ACCOUNTS_PER_IX: usize = 64;

struct AuthorizedPdaSigner {
    authority: Address,
    signer_pda: Address,
    bump_seed: [u8; 1],
}

#[inline(never)]
pub fn process_submit<S: SigningScheme>(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
    submit: S::Submit,
) -> ProgramResult
where
    SchemeState<S>: for<'de> SchemaRead<'de, DefaultConfig, Dst = SchemeState<S>>
        + SchemaWrite<DefaultConfig, Src = SchemeState<S>>,
{
    let [
        durable_signer_account,
        slot_hashes_account,
        instructions_sysvar,
        remaining_accounts @ ..,
    ] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if slot_hashes_account.address() != &slot_hashes_sysvar_id::ID {
        return Err(ProgramError::InvalidArgument);
    }

    if instructions_sysvar.address() != &instructions_sysvar_id::ID {
        return Err(DurableSignerError::InvalidInstructionsSysvar.into());
    }

    if !is_transaction_level() {
        return Err(DurableSignerError::OuterTxMustContainOnlySubmit.into());
    }

    // Submit must be the current top-level instruction. This rejects CPI callers
    // and batched outer transactions, which would have different replay semantics.
    let instructions = Instructions::try_from(&*instructions_sysvar)
        .map_err(|_| ProgramError::from(DurableSignerError::InvalidInstructionsSysvar))?;
    if instructions.num_instructions() != 1 || instructions.load_current_index() != 0 {
        return Err(DurableSignerError::OuterTxMustContainOnlySubmit.into());
    }

    let top_level = instructions
        .load_instruction_at(0)
        .map_err(|_| ProgramError::from(DurableSignerError::InvalidInstructionsSysvar))?;
    if top_level.get_program_id() != program_id
        || !bytes_match(top_level.get_instruction_data(), instruction_data)
    {
        return Err(DurableSignerError::OuterTxMustContainOnlySubmit.into());
    }

    let message = S::message(&submit);
    message
        .sanitize()
        .map_err(|_| ProgramError::from(DurableSignerError::InvalidWrappedTransaction))?;
    let account_keys = message.static_account_keys();

    // Signed transaction config is not replayed by CPI, so fee and compute
    // policy belongs on the outer Submit transaction.
    if matches!(
        message,
        VersionedMessage::V1(v1)
            if v1.config.priority_fee.is_some()
                || v1.config.compute_unit_limit.is_some()
                || v1.config.loaded_accounts_data_size_limit.is_some()
                || v1.config.heap_size.is_some()
    ) {
        return Err(DurableSignerError::InvalidWrappedTransaction.into());
    }
    if matches!(message, VersionedMessage::V0(v0) if !v0.address_table_lookups.is_empty()) {
        return Err(DurableSignerError::InvalidWrappedTransaction.into());
    }

    let signer_count = usize::from(message.header().num_required_signatures);
    let signer_keys = account_keys.get(..signer_count).ok_or(ProgramError::from(
        DurableSignerError::InvalidWrappedTransaction,
    ))?;
    S::validate_submit(&submit, signer_count)?;

    let authority_account_count = S::authority_account_count(signer_count)?;
    let expected_remaining_accounts = authority_account_count
        .checked_add(account_keys.len())
        .ok_or(ProgramError::InvalidArgument)?;
    if remaining_accounts.len() != expected_remaining_accounts {
        return Err(DurableSignerError::WrappedMessageAccountsMismatch.into());
    }

    let (authority_accounts, wrapped_accounts) =
        remaining_accounts.split_at(authority_account_count);
    verify_wrapped_accounts(wrapped_accounts, account_keys)?;

    if !durable_signer_account.owned_by(program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    let data = durable_signer_account.try_borrow()?;
    let mut state: SchemeState<S> = wincode::deserialize_exact(&data)
        .map_err(|_| DurableSignerError::InvalidDurableSignerAccount)?;
    drop(data);

    if message.recent_blockhash().as_ref() != state.nonce.as_ref() {
        return Err(DurableSignerError::NonceMismatch.into());
    }

    let message_bytes = message.serialize();

    let authorized_signers = authorize_required_signers::<S>(
        program_id,
        authority_accounts,
        signer_keys,
        &state.authority,
        &submit,
        &message_bytes,
    )?;

    execute_wrapped_message(wrapped_accounts, message, &authorized_signers)?;

    // The nonce is consumed only after every wrapped CPI succeeds.
    let slot_hashes = SlotHashes::from_account_view(&*slot_hashes_account)
        .map_err(|_| ProgramError::from(DurableSignerError::SlotHashesUnavailable))?;
    let recent = slot_hashes.get_entry(0).ok_or(ProgramError::from(
        DurableSignerError::SlotHashesUnavailable,
    ))?;
    let durable_signer_address = address_value(durable_signer_account.address());
    let old_nonce = state.nonce;
    let message_hash = solana_sha256_hasher::hash(&message_bytes);

    state.nonce = solana_sha256_hasher::hashv(&[
        NONCE_DERIVATION_TAG,
        durable_signer_address.as_ref(),
        old_nonce.as_ref(),
        &recent.hash,
        message_hash.as_ref(),
    ]);

    if durable_signer_account.data_len() != S::STATE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut data = durable_signer_account.try_borrow_mut()?;
    wincode::serialize_into(&mut *data, &state)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok(())
}

fn verify_wrapped_accounts(
    wrapped_accounts: &[AccountView],
    account_keys: &[Address],
) -> Result<(), ProgramError> {
    if wrapped_accounts.len() != account_keys.len() {
        return Err(DurableSignerError::WrappedMessageAccountsMismatch.into());
    }
    for (account, expected) in wrapped_accounts.iter().zip(account_keys) {
        if account.address() != expected {
            return Err(DurableSignerError::WrappedMessageAccountsMismatch.into());
        }
    }
    Ok(())
}

fn authorize_required_signers<S: SigningScheme>(
    program_id: &Address,
    authority_accounts: &[AccountView],
    signer_keys: &[Address],
    state_authority: &S::Authority,
    submit: &S::Submit,
    message_bytes: &[u8],
) -> Result<Vec<AuthorizedPdaSigner>, ProgramError> {
    let mut saw_state_authority = false;
    let mut authorized = Vec::with_capacity(signer_keys.len());
    for (signer_index, expected_pda) in signer_keys.iter().enumerate() {
        let VerifiedSigner {
            authority,
            bump,
            is_state_authority,
        } = S::verify_signer(
            program_id,
            state_authority,
            authority_accounts,
            signer_index,
            expected_pda,
            submit,
            message_bytes,
        )?;
        if is_state_authority {
            saw_state_authority = true;
        }

        authorized.push(AuthorizedPdaSigner {
            authority,
            signer_pda: *expected_pda,
            bump_seed: [bump],
        });
    }

    if !saw_state_authority {
        return Err(DurableSignerError::AuthorityMismatch.into());
    }

    Ok(authorized)
}

fn execute_wrapped_message(
    wrapped_accounts: &[AccountView],
    message: &VersionedMessage,
    authorized_signers: &[AuthorizedPdaSigner],
) -> ProgramResult {
    let account_keys = message.static_account_keys();
    let instructions = message.instructions();
    let signer_seed_sets: Vec<[Seed; 3]> = authorized_signers
        .iter()
        .map(|signer| {
            [
                Seed::from(DurableSignerPda::SEED_PREFIX),
                Seed::from(signer.authority.as_ref()),
                Seed::from(&signer.bump_seed),
            ]
        })
        .collect();
    let cpi_signers: Vec<Signer> = signer_seed_sets.iter().map(Signer::from).collect();

    for compiled in instructions {
        let program_index = usize::from(compiled.program_id_index);
        if program_index >= account_keys.len() {
            return Err(DurableSignerError::InvalidWrappedTransaction.into());
        }
        if compiled.accounts.len() > MAX_CPI_ACCOUNTS_PER_IX {
            return Err(DurableSignerError::InvalidWrappedTransaction.into());
        }

        let program_account = wrapped_account(wrapped_accounts, program_index)?;
        let mut instruction_accounts = Vec::with_capacity(compiled.accounts.len());
        let mut account_views = Vec::with_capacity(compiled.accounts.len());

        for account_index in &compiled.accounts {
            let account_index = usize::from(*account_index);
            if account_index >= account_keys.len() {
                return Err(DurableSignerError::InvalidWrappedTransaction.into());
            }
            let account = wrapped_account(wrapped_accounts, account_index)?;
            let declared_signer = message.is_signer(account_index);
            let is_durable_signer = authorized_signers
                .iter()
                .any(|signer| account.address() == &signer.signer_pda);
            let is_signer = declared_signer && is_durable_signer;
            if declared_signer && !is_signer {
                return Err(DurableSignerError::MissingRequiredSigner.into());
            }

            instruction_accounts.push(InstructionAccount::new(
                account.address(),
                is_cpi_writable(message, account_index),
                is_signer,
            ));
            account_views.push(account);
        }

        let view = InstructionView {
            program_id: program_account.address(),
            accounts: instruction_accounts.as_slice(),
            data: compiled.data.as_slice(),
        };

        invoke_signed_with_bounds::<MAX_CPI_ACCOUNTS_PER_IX, &AccountView>(
            &view,
            account_views.as_slice(),
            cpi_signers.as_slice(),
        )?;
    }

    Ok(())
}

fn wrapped_account(
    wrapped_accounts: &[AccountView],
    account_index: usize,
) -> Result<&AccountView, ProgramError> {
    wrapped_accounts
        .get(account_index)
        .ok_or(DurableSignerError::WrappedMessageAccountsMismatch.into())
}

fn bytes_match(left: &[u8], right: &[u8]) -> bool {
    // Slice equality lowers to `memcmp`, which is not available in the SBF build.
    if left.len() != right.len() {
        return false;
    }
    for (left_byte, right_byte) in left.iter().zip(right) {
        if left_byte != right_byte {
            return false;
        }
    }
    true
}

fn is_cpi_writable(message: &VersionedMessage, index: usize) -> bool {
    // Keep this aligned with Solana's message writability rules before building CPI metas.
    let header = message.header();
    let account_keys = message.static_account_keys();
    let required_signatures = usize::from(header.num_required_signatures);
    let readonly_signed = usize::from(header.num_readonly_signed_accounts);
    let readonly_unsigned = usize::from(header.num_readonly_unsigned_accounts);
    let requested_writable = index < required_signatures.saturating_sub(readonly_signed)
        || (index >= required_signatures
            && index < account_keys.len().saturating_sub(readonly_unsigned));

    requested_writable
        && (!is_called_as_program(message.instructions(), index)
            || uses_upgradeable_loader(account_keys))
}

fn is_called_as_program(instructions: &[CompiledInstruction], key_index: usize) -> bool {
    let Ok(key_index) = u8::try_from(key_index) else {
        return false;
    };
    instructions
        .iter()
        .any(|instruction| instruction.program_id_index == key_index)
}

fn uses_upgradeable_loader(account_keys: &[Address]) -> bool {
    account_keys
        .iter()
        .any(|key| key == &bpf_loader_upgradeable::ID)
}

fn address_value(address: &Address) -> Address {
    Address::from(address)
}

fn is_transaction_level() -> bool {
    #[cfg(target_os = "solana")]
    {
        get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT
    }

    #[cfg(not(target_os = "solana"))]
    {
        true
    }
}
