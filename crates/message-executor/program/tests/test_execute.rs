use {
    crate::helpers::{
        common::{
            compiled_transfer_instruction, decode_state, init_mollusk, initialize_nonce_account,
            message_hash, system_account, system_transfer_instruction,
        },
        execute_builder::ExecuteBuilder,
        nonce_account_builder::NonceAccountBuilder,
    },
    mollusk_svm::result::Check,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, error::InstructionError},
    solana_message::{
        Message, MessageHeader, VersionedMessage, v0,
        v1::{Message as MessageV1, TransactionConfig},
    },
    solana_program_error::ProgramError,
    spl_message_executor_client::instruction::execute,
    spl_message_executor_interface::error::Error as MessageExecutorError,
    spl_nonce_client::instruction::advance,
    spl_nonce_interface::{error::Error as NonceError, state::NONCE_DERIVATION_TAG},
};

pub mod helpers;

fn address(tag: u8) -> Address {
    Address::from([tag; 32])
}

fn executor_error(error: MessageExecutorError) -> ProgramError {
    ProgramError::Custom(error as u32)
}

#[test]
fn execute_transfers_and_advances_nonce() {
    let authority = address(3);
    let recipient = Address::new_unique();
    let transfer_lamports = 1_000_000;

    let result = ExecuteBuilder::default()
        .authority(authority)
        .inner_instruction(system_transfer_instruction(
            authority,
            recipient,
            transfer_lamports,
        ))
        .execute();

    assert_eq!(
        result.account(&recipient).unwrap().lamports,
        transfer_lamports
    );
}

#[test]
fn execute_advances_nonce_with_program_derivation() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let old_nonce = decode_state(&nonce_account.1).nonce;

    let result = ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account.clone())
        .execute();

    let new_state = decode_state(&result.nonce_account.1);
    let slot_hash = mollusk.sysvars.slot_hashes.first().unwrap().1;
    assert_eq!(
        new_state.nonce,
        solana_sha256_hasher::hashv(&[
            NONCE_DERIVATION_TAG,
            nonce_account.0.as_ref(),
            old_nonce.as_ref(),
            slot_hash.as_ref(),
        ])
    );
}

#[test]
fn execute_rejects_replay_after_nonce_advances() {
    let authority = address(3);
    let first = ExecuteBuilder::default().authority(authority).execute();

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(first.nonce_account)
        .message(first.message)
        .check(Check::err(executor_error(
            MessageExecutorError::NonceMismatch,
        )))
        .execute();
}

#[test]
fn execute_rejects_stale_recent_blockhash() {
    ExecuteBuilder::default()
        .recent_blockhash(Hash::new_from_array([9; 32]))
        .check(Check::err(executor_error(
            MessageExecutorError::NonceMismatch,
        )))
        .execute();
}

#[test]
fn execute_rejects_missing_required_signer_privilege() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    // The authority is the first message account, after the three fixed accounts.
    assert_eq!(instruction.accounts[3].pubkey, authority);
    instruction.accounts[3].is_signer = false;

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(executor_error(
            MessageExecutorError::MissingRequiredSigner,
        )))
        .execute();
}

#[test]
fn execute_rejects_authority_not_among_signers() {
    let authority = address(3);
    let other_signer = address(4);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&other_signer),
        &message_hash(nonce),
    ));

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .account(other_signer, system_account(0))
        .check(Check::err(executor_error(
            MessageExecutorError::AuthorityMismatch,
        )))
        .execute();
}

#[test]
fn execute_rejects_missing_fixed_accounts() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    instruction.accounts.truncate(1); // drop the other fixed accounts and the message accounts

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(ProgramError::NotEnoughAccountKeys))
        .execute();
}

#[test]
fn execute_rejects_nonce_account_owned_by_another_program() {
    ExecuteBuilder::default()
        .nonce_account(
            NonceAccountBuilder::new()
                .key(Address::new_unique())
                .owner(Address::new_unique())
                .build(),
        )
        .recent_blockhash(Hash::new_from_array([7; 32]))
        .check(Check::err(ProgramError::IllegalOwner))
        .execute();
}

#[test]
fn execute_rejects_uninitialized_nonce_account() {
    // Program-owned but with truncated data that cannot parse as nonce state.
    let malformed = NonceAccountBuilder::new()
        .key(Address::new_unique())
        .data(vec![0; 1])
        .build();

    ExecuteBuilder::default()
        .nonce_account(malformed)
        .recent_blockhash(Hash::new_from_array([7; 32]))
        .check(Check::err(executor_error(
            MessageExecutorError::InvalidNonceAccount,
        )))
        .execute();
}

#[test]
fn execute_rejects_readonly_nonce_account() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    instruction.accounts[0] = AccountMeta::new_readonly(nonce_account.0, false);

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::instruction_err(
            InstructionError::PrivilegeEscalation,
        ))
        .execute();
}

#[test]
fn execute_rejects_wrong_slot_hashes_account() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    let wrong_slot_hashes = Address::new_unique();
    instruction.accounts[2] = AccountMeta::new_readonly(wrong_slot_hashes, false);

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(ProgramError::InvalidArgument))
        .execute();
}

#[test]
fn execute_rejects_wrong_nonce_program_account() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    let wrong_nonce_program = Address::new_unique();
    instruction.accounts[1] = AccountMeta::new_readonly(wrong_nonce_program, false);

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(ProgramError::IncorrectProgramId))
        .execute();
}

#[test]
fn execute_accepts_v0_message_without_lookups() {
    let authority = address(3);
    let recipient = Address::new_unique();
    let transfer_lamports = 1_000_000;
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::V0(v0::Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        account_keys: vec![authority, recipient, solana_system_interface::program::id()],
        recent_blockhash: message_hash(nonce),
        instructions: vec![compiled_transfer_instruction(0, 1, 2, transfer_lamports)],
        address_table_lookups: vec![],
    });

    let result = ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute();

    assert_eq!(
        result.account(&recipient).unwrap().lamports,
        transfer_lamports
    );
}

#[test]
fn execute_rejects_v0_address_table_lookups() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::V0(v0::Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        account_keys: vec![authority],
        recent_blockhash: message_hash(nonce),
        instructions: vec![],
        address_table_lookups: vec![v0::MessageAddressTableLookup {
            account_key: Address::new_unique(),
            writable_indexes: vec![0],
            readonly_indexes: vec![],
        }],
    });

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .check(Check::err(executor_error(
            MessageExecutorError::InvalidMessage,
        )))
        .execute();
}

#[test]
fn execute_accepts_v1_message_with_empty_config() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::V1(MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        config: TransactionConfig::empty(),
        lifetime_specifier: message_hash(nonce),
        account_keys: vec![authority],
        instructions: vec![],
    });

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute();
}

#[test]
fn execute_rejects_v1_message_with_transaction_config() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::V1(MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        config: TransactionConfig::empty().with_compute_unit_limit(100_000),
        lifetime_specifier: message_hash(nonce),
        account_keys: vec![authority],
        instructions: vec![],
    });

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .check(Check::err(executor_error(
            MessageExecutorError::InvalidMessage,
        )))
        .execute();
}

#[test]
fn execute_rejects_duplicate_account_keys() {
    let authority = address(3);
    let duplicate = Address::new_unique();
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::Legacy(Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 2,
        },
        account_keys: vec![authority, duplicate, duplicate],
        recent_blockhash: message_hash(nonce),
        instructions: vec![],
    });

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .check(Check::err(executor_error(
            MessageExecutorError::InvalidMessage,
        )))
        .execute();
}

#[test]
fn execute_rejects_invalid_instruction_indexes() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;

    let message = VersionedMessage::Legacy(Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        account_keys: vec![authority],
        recent_blockhash: message_hash(nonce),
        instructions: vec![compiled_transfer_instruction(0, 1, 9, 1)],
    });

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .check(Check::err(executor_error(
            MessageExecutorError::InvalidMessage,
        )))
        .execute();
}

#[test]
fn execute_rejects_account_order_mismatch() {
    let authority = address(3);
    let recipient = Address::new_unique();
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[crate::helpers::execute_builder::message_instruction(
            system_transfer_instruction(authority, recipient, 1),
        )],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    instruction.accounts.swap(3, 4); // swap two message accounts out of order

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(executor_error(
            MessageExecutorError::MessageAccountsMismatch,
        )))
        .execute();
}

#[test]
fn execute_rejects_account_count_mismatch() {
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut extra = execute(&nonce_account.0, &message);
    extra
        .accounts
        .push(AccountMeta::new_readonly(Address::new_unique(), false));

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(extra)
        .check(Check::err(executor_error(
            MessageExecutorError::MessageAccountsMismatch,
        )))
        .execute();
}

#[test]
fn execute_rejects_readonly_message_writable_account() {
    let authority = address(3);
    let recipient = Address::new_unique();
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce = decode_state(&nonce_account.1).nonce;
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[crate::helpers::execute_builder::message_instruction(
            system_transfer_instruction(authority, recipient, 1),
        )],
        Some(&authority),
        &message_hash(nonce),
    ));

    let mut instruction = execute(&nonce_account.0, &message);
    // The recipient is writable in the message. Pass it readonly.
    assert_eq!(instruction.accounts[4].pubkey, recipient);
    instruction.accounts[4] = AccountMeta::new_readonly(recipient, false);

    ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .message(message)
        .execute_instruction(instruction)
        .check(Check::err(executor_error(
            MessageExecutorError::MessageAccountsMismatch,
        )))
        .execute();
}

#[test]
fn execute_does_not_advance_nonce_when_cpi_fails() {
    let authority = address(3);
    let recipient = Address::new_unique();
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let old_nonce = decode_state(&nonce_account.1).nonce;

    let overdraft = 1_000_000_000;
    let result = ExecuteBuilder::default()
        .authority(authority)
        .authority_lamports(1)
        .nonce_account(nonce_account)
        .inner_instruction(system_transfer_instruction(authority, recipient, overdraft))
        // The system program's negative-lamports error propagates through the CPI.
        .check(Check::err(ProgramError::Custom(1)))
        .execute();

    assert_eq!(decode_state(&result.nonce_account.1).nonce, old_nonce);
}

#[test]
fn execute_batches_multiple_inner_instructions() {
    let authority = address(3);
    let first_recipient = Address::new_unique();
    let second_recipient = Address::new_unique();

    let result = ExecuteBuilder::default()
        .authority(authority)
        .inner_instruction(system_transfer_instruction(authority, first_recipient, 100))
        .inner_instruction(system_transfer_instruction(
            authority,
            second_recipient,
            200,
        ))
        .execute();

    assert_eq!(result.account(&first_recipient).unwrap().lamports, 100);
    assert_eq!(result.account(&second_recipient).unwrap().lamports, 200);
}

#[test]
fn execute_accepts_multiple_signers() {
    let authority = address(3);
    let second_signer = address(4);
    let recipient = Address::new_unique();

    let result = ExecuteBuilder::default()
        .authority(authority)
        .inner_instruction(system_transfer_instruction(authority, recipient, 100))
        .inner_instruction(system_transfer_instruction(second_signer, recipient, 200))
        .account(second_signer, system_account(1_000_000))
        .execute();

    assert_eq!(result.account(&recipient).unwrap().lamports, 300);
}

#[test]
fn execute_reverts_when_message_consumes_its_own_nonce() {
    // The wrapped message advances the nonce itself, attempting a double spend. The final
    // consume re-checks the value the executor verified, so the whole transaction reverts.
    let authority = address(3);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let nonce_address = nonce_account.0;
    let old_nonce = decode_state(&nonce_account.1).nonce;

    let result = ExecuteBuilder::default()
        .authority(authority)
        .nonce_account(nonce_account)
        .inner_instruction(advance(&nonce_address, &authority, old_nonce))
        .check(Check::err(ProgramError::Custom(
            NonceError::NonceMismatch as u32,
        )))
        .execute();

    assert_eq!(decode_state(&result.nonce_account.1).nonce, old_nonce);
}
