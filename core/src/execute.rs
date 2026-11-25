use {
    crate::{
        cpi::invoke_instructions,
        validate::{validate_execution_message, validate_message_accounts},
    },
    pinocchio::{
        AccountView, ProgramResult,
        cpi::{Seed, Signer},
        error::ProgramError,
    },
    solana_message::VersionedMessage,
    spl_message_executor_interface::{
        error::Error, instruction::derive_transition_commitment, pda::TransitionSigner,
    },
    spl_nonce_client::cpi::Advance,
    spl_nonce_interface::state::Nonce,
};

pub fn process_execute(
    accounts: &mut [AccountView],
    execution_message: VersionedMessage,
) -> ProgramResult {
    let [
        nonce_authority_account,
        nonce_account,
        nonce_program,
        transition_signer,
        message_accounts @ ..,
    ] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if nonce_program.address() != &spl_nonce_interface::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !nonce_account.owned_by(&spl_nonce_interface::ID) {
        return Err(ProgramError::IllegalOwner);
    }
    let nonce_data = nonce_account.try_borrow()?;
    let Nonce { nonce, .. } = Nonce::view(&nonce_data).map_err(|_| Error::InvalidNonceAccount)?;

    // If the authority is our PDA, `invoke_signed` would otherwise supply its approval
    if !nonce_authority_account.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // A caller's own signer would pass runtime checks but derive a different successor
    if transition_signer.address() != &TransitionSigner::ADDRESS {
        return Err(Error::InvalidTransitionSigner.into());
    }

    let message = validate_execution_message(&execution_message)?;

    if &message.lifetime_specifier != nonce {
        return Err(Error::NonceMismatch.into());
    }

    validate_message_accounts(message_accounts, message)?;

    // The advance ix requires an owned nonce
    let current_nonce = *nonce;
    drop(nonce_data);

    let seeds = TransitionSigner::SIGNER_SEEDS.map(Seed::from);
    let signers = &[Signer::from(&seeds)];

    // Consume the nonce before invoking the message instructions to prevent recursive execution.
    // The advance rolls back if any instruction invocation fails.
    Advance {
        authority: nonce_authority_account,
        transition_signer,
        nonce_account,
        current_nonce,
        transition_commitment: derive_transition_commitment(&execution_message),
    }
    .invoke_signed(signers)?;

    invoke_instructions(message_accounts, message)
}

use {
    alloc::{collections::BTreeSet, vec::Vec},
    pinocchio::{
        AccountView, ProgramResult,
        cpi::invoke_with_slice,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_message::v1,
    spl_message_executor_interface::error::Error,
};

pub fn invoke_instructions(
    message_accounts: &[AccountView],
    execution_message: &v1::Message,
) -> ProgramResult {
    // Allocate once for the largest instruction and reuse across the execution
    let max_instruction_accounts = execution_message
        .instructions
        .iter()
        .map(|ix| ix.accounts.len())
        .max()
        .unwrap_or_default();
    let mut instruction_accounts = Vec::with_capacity(max_instruction_accounts);
    let mut account_views = Vec::with_capacity(max_instruction_accounts);

    // Invoke each instruction via CPI
    for ix in &execution_message.instructions {
        instruction_accounts.clear();
        account_views.clear();

        // Resolve the invoked program
        let program_account = message_accounts
            .get(usize::from(ix.program_id_index))
            .ok_or(Error::InvalidMessage)?;

        // Rebuild CPI accounts with exactly the message requested privileges
        for idx in &ix.accounts {
            let account_index = usize::from(*idx);
            let message_account = message_accounts
                .get(account_index)
                .ok_or(Error::InvalidMessage)?;
            let is_writable = execution_message
                .is_maybe_writable_with_reserved_addresses(account_index, None::<&BTreeSet<_>>);
            let is_signer = execution_message.is_signer(account_index);

            instruction_accounts.push(InstructionAccount::new(
                message_account.address(),
                is_writable,
                is_signer,
            ));
            account_views.push(message_account);
        }

        let ix_view = InstructionView {
            program_id: program_account.address(),
            accounts: instruction_accounts.as_slice(),
            data: ix.data.as_slice(),
        };

        invoke_with_slice::<&AccountView>(&ix_view, account_views.as_slice())?;
    }

    Ok(())
}
