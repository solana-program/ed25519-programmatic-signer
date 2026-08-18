extern crate alloc;

use {
    crate::helpers::{
        common::{decode_state, init_mollusk, initialize_nonce_account},
        execute_builder::{DEFAULT_AUTHORITY, ExecuteBuilder},
        nonce_account_builder::NonceAccountBuilder,
    },
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, error::InstructionError},
    solana_message::{
        Message, MessageHeader, VersionedMessage,
        compiled_instruction::CompiledInstruction,
        v0,
        v1::{Message as MessageV1, TransactionConfig},
    },
    solana_program_error::ProgramError,
    solana_system_interface::instruction::transfer,
    spl_message_executor_interface::error::Error as MessageExecutorError,
    spl_nonce_client::instruction::advance,
    spl_nonce_interface::error::Error as NonceError,
};

pub mod helpers;

#[test]
fn execute_rejects_missing_fixed_accounts() {
    ExecuteBuilder::default()
        .mutate_execute_ix(|ix| ix.accounts.truncate(1))
        .check_err(ProgramError::NotEnoughAccountKeys)
        .execute();
}

#[test]
fn execute_rejects_wrong_nonce_program_account() {
    let wrong_nonce_program = Address::new_unique();
    ExecuteBuilder::default()
        .mutate_execute_ix(move |ix| ix.accounts[1].pubkey = wrong_nonce_program)
        .check_err(ProgramError::IncorrectProgramId)
        .execute();
}

#[test]
fn execute_rejects_nonce_account_owned_by_another_program() {
    let (nonce_address, nonce_account) = NonceAccountBuilder::new()
        .owner(Address::new_unique())
        .build();

    ExecuteBuilder::default()
        .nonce_account(nonce_address, nonce_account)
        .check_err(ProgramError::IllegalOwner)
        .execute();
}

#[test]
fn execute_rejects_wrong_slot_hashes_account() {
    let wrong_slot_hashes = Address::new_unique();
    ExecuteBuilder::default()
        .mutate_execute_ix(move |ix| ix.accounts[2].pubkey = wrong_slot_hashes)
        .check_err(ProgramError::UnsupportedSysvar)
        .execute();
}

#[test]
fn execute_rejects_malformed_nonce_account() {
    // Program owned but with truncated data that cannot parse as nonce state
    let (nonce_address, nonce_account) = NonceAccountBuilder::new().data(vec![0; 1]).build();

    ExecuteBuilder::default()
        .nonce_account(nonce_address, nonce_account)
        .check_err(MessageExecutorError::InvalidNonceAccount)
        .execute();
}

#[test]
fn execute_rejects_v0_address_table_lookups() {
    let message = VersionedMessage::V0(v0::Message {
        address_table_lookups: vec![v0::MessageAddressTableLookup::default()],
        ..v0::Message::default()
    });

    ExecuteBuilder::default()
        .message(message)
        .check_err(MessageExecutorError::InvalidMessage)
        .execute();
}

#[test]
fn execute_rejects_v1_message_with_transaction_config() {
    let message = VersionedMessage::V1(MessageV1 {
        config: TransactionConfig::empty().with_compute_unit_limit(100_000),
        ..MessageV1::default()
    });

    ExecuteBuilder::default()
        .message(message)
        .check_err(MessageExecutorError::InvalidMessage)
        .execute();
}

fn legacy_message_mut(message: &mut VersionedMessage) -> &mut Message {
    let VersionedMessage::Legacy(message) = message else {
        panic!("expected legacy message");
    };
    message
}

#[test]
fn execute_rejects_message_that_fails_sanitization() {
    ExecuteBuilder::default()
        .mutate_message(|message| {
            legacy_message_mut(message).header.num_required_signatures = 2;
        })
        .check_err(MessageExecutorError::InvalidMessage)
        .execute();
}

#[test]
fn execute_rejects_duplicate_message_addresses() {
    ExecuteBuilder::default()
        .mutate_message(|message| {
            let message = legacy_message_mut(message);
            message.header.num_readonly_unsigned_accounts = 1;
            message.account_keys.push(DEFAULT_AUTHORITY);
        })
        .check_err(MessageExecutorError::InvalidMessage)
        .execute();
}

#[test]
fn execute_rejects_recent_blockhash_mismatch() {
    ExecuteBuilder::default()
        .recent_blockhash(Hash::new_from_array([9; 32]))
        .check_err(MessageExecutorError::NonceMismatch)
        .execute();
}

#[test]
fn execute_rejects_message_reuse_after_nonce_advances() {
    let first = ExecuteBuilder::default().execute();

    ExecuteBuilder::default()
        .nonce_account(first.nonce_address, first.nonce_account)
        .message(first.message)
        .check_err(MessageExecutorError::NonceMismatch)
        .execute();
}

#[test]
fn execute_rejects_account_count_mismatch() {
    ExecuteBuilder::default()
        .mutate_execute_ix(|ix| {
            ix.accounts
                .push(AccountMeta::new_readonly(Address::new_unique(), false));
        })
        .check_err(MessageExecutorError::MessageAccountsMismatch)
        .execute();
}

#[test]
fn execute_rejects_account_order_mismatch() {
    ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &Address::new_unique(), 1))
        .mutate_execute_ix(|ix| ix.accounts.swap(3, 4))
        .check_err(MessageExecutorError::MessageAccountsMismatch)
        .execute();
}

#[test]
fn execute_rejects_readonly_message_writable_account() {
    ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &Address::new_unique(), 1))
        .mutate_execute_ix(|ix| ix.accounts[4].is_writable = false)
        .check(Check::instruction_err(
            InstructionError::PrivilegeEscalation,
        ))
        .execute();
}

#[test]
fn execute_rejects_missing_required_signer_privilege() {
    ExecuteBuilder::default()
        .mutate_execute_ix(|ix| ix.accounts[3].is_signer = false)
        .check(Check::instruction_err(
            InstructionError::PrivilegeEscalation,
        ))
        .execute();
}

#[test]
fn execute_rejects_authority_not_among_signers() {
    let wrong_signer = Address::new_unique();

    ExecuteBuilder::default()
        .mutate_message(move |message| {
            let message = legacy_message_mut(message);
            message.account_keys[0] = wrong_signer;
        })
        .check_err(MessageExecutorError::MissingNonceAuthoritySigner)
        .execute();
}

#[test]
fn execute_rejects_readonly_nonce_account() {
    ExecuteBuilder::default()
        .mutate_execute_ix(|ix| ix.accounts[0].is_writable = false)
        .check(Check::instruction_err(
            InstructionError::PrivilegeEscalation,
        ))
        .execute();
}

#[test]
fn execute_rejects_uninitialized_nonce_account() {
    let (nonce_address, nonce_account) = NonceAccountBuilder::new().build();

    ExecuteBuilder::default()
        .authority(Address::default())
        .nonce_account(nonce_address, nonce_account)
        .check_err(NonceError::InvalidNonceAccount)
        .execute();
}

#[test]
fn execute_downgrades_extra_writable_privilege_for_inner_cpi() {
    // Verifies extra writable privilege on the outer executor instruction
    // does not leak into the inner CPI
    let recipient = Address::new_unique();

    ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &recipient, 1))
        .mutate_message(|message| {
            // makes the recipient readonly in the wrapped message
            let message = legacy_message_mut(message);
            message.header.num_readonly_unsigned_accounts = 2;
        })
        // gives recipient extra writable privilege on the outer ix
        .mutate_execute_ix(|ix| ix.accounts[4].is_writable = true)
        .check(Check::instruction_err(
            InstructionError::ReadonlyLamportChange,
        ))
        .execute();
}

#[test]
fn execute_downgrades_extra_signer_privilege_for_inner_cpi() {
    // Verifies extra signer privilege on the outer executor instruction
    // does not leak into the inner CPI
    let source = Address::new_unique();
    let recipient = Address::new_unique();

    ExecuteBuilder::default()
        .inner_instruction(transfer(&source, &recipient, 1))
        .mutate_message(|message| {
            // makes the source a nonsigner in the wrapped message
            let message = legacy_message_mut(message);
            message.header.num_required_signatures = 1;
        })
        // gives source extra signer privilege on the outer ix
        .mutate_execute_ix(|ix| ix.accounts[4].is_signer = true)
        .account(
            source,
            Account::new(1, 0, &solana_system_interface::program::id()),
        )
        .check_err(ProgramError::MissingRequiredSignature)
        .execute();
}

#[test]
fn execute_rolls_back_nonce_when_inner_instruction_fails() {
    let recipient = Address::new_unique();

    let overdraft = 1_000_000_000;
    let result = ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &recipient, overdraft))
        .check_err(ProgramError::Custom(1))
        .execute();

    assert_eq!(
        decode_state(&result.nonce_account).nonce,
        *result.message.recent_blockhash()
    );
}

#[test]
fn execute_rejects_message_that_consumes_its_own_nonce() {
    let mollusk = init_mollusk();
    let (nonce_address, nonce_account) = initialize_nonce_account(&mollusk, &DEFAULT_AUTHORITY);
    let old_nonce = decode_state(&nonce_account).nonce;

    ExecuteBuilder::new(mollusk)
        .nonce_account(nonce_address, nonce_account)
        .inner_instruction(advance(&DEFAULT_AUTHORITY, &nonce_address, old_nonce))
        .check_err(NonceError::NonceMismatch)
        .execute();
}

#[test]
fn execute_accepts_nonce_authority_as_readonly_nonpayer_signer() {
    let payer = Address::new_unique();

    ExecuteBuilder::default()
        .mutate_message(move |message| {
            // Insert a writable payer before the authority,
            // making the authority the second, readonly signer
            let message = legacy_message_mut(message);
            message.header.num_required_signatures = 2;
            message.header.num_readonly_signed_accounts = 1;
            message.account_keys.insert(0, payer);
        })
        .execute();
}

#[test]
fn execute_accepts_static_legacy_message() {
    assert_static_message_executes(|header, addresses, recent_blockhash, instructions| {
        VersionedMessage::Legacy(Message {
            header,
            account_keys: addresses,
            recent_blockhash,
            instructions,
        })
    });
}

#[test]
fn execute_accepts_static_v0_message() {
    assert_static_message_executes(|header, addresses, recent_blockhash, instructions| {
        VersionedMessage::V0(v0::Message {
            header,
            account_keys: addresses,
            recent_blockhash,
            instructions,
            address_table_lookups: vec![],
        })
    });
}

#[test]
fn execute_accepts_static_v1_message() {
    assert_static_message_executes(|header, addresses, recent_blockhash, instructions| {
        VersionedMessage::V1(MessageV1 {
            header,
            config: TransactionConfig::empty(),
            lifetime_specifier: recent_blockhash,
            account_keys: addresses,
            instructions,
        })
    });
}

fn assert_static_message_executes(
    build_message: impl FnOnce(
        MessageHeader,
        Vec<Address>,
        Hash,
        Vec<CompiledInstruction>,
    ) -> VersionedMessage,
) {
    let recipient = Address::new_unique();
    let transfer_lamports = 1_000_000;
    let mollusk = init_mollusk();
    let (nonce_address, nonce_account) = initialize_nonce_account(&mollusk, &DEFAULT_AUTHORITY);
    let old_nonce = decode_state(&nonce_account).nonce;
    let transfer_ix = transfer(&DEFAULT_AUTHORITY, &recipient, transfer_lamports);
    let message = build_message(
        MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        vec![
            DEFAULT_AUTHORITY,
            recipient,
            solana_system_interface::program::id(),
        ],
        old_nonce,
        vec![CompiledInstruction::new_from_raw_parts(
            2,
            transfer_ix.data,
            vec![0, 1],
        )],
    );

    let result = ExecuteBuilder::new(mollusk)
        .nonce_account(nonce_address, nonce_account)
        .message(message)
        .execute();

    assert_eq!(
        result.account(&recipient).unwrap().lamports,
        transfer_lamports
    );
    assert_ne!(decode_state(&result.nonce_account).nonce, old_nonce);
}

#[test]
fn execute_empty_message_advances_nonce() {
    // An empty message must still consume the nonce
    let mollusk = init_mollusk();
    let (nonce_address, nonce_account) = initialize_nonce_account(&mollusk, &DEFAULT_AUTHORITY);
    let old_nonce = decode_state(&nonce_account).nonce;

    let result = ExecuteBuilder::new(mollusk)
        .nonce_account(nonce_address, nonce_account)
        .execute();

    assert_ne!(decode_state(&result.nonce_account).nonce, old_nonce);
}

#[test]
fn execute_batches_instructions_from_multiple_signers() {
    let second_signer = Address::new_unique();
    let first_recipient = Address::new_unique();
    let second_recipient = Address::new_unique();

    let result = ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &first_recipient, 100))
        .inner_instruction(transfer(&second_signer, &second_recipient, 200))
        .account(
            second_signer,
            Account::new(1_000_000, 0, &solana_system_interface::program::id()),
        )
        .execute();

    assert_eq!(result.account(&first_recipient).unwrap().lamports, 100);
    assert_eq!(result.account(&second_recipient).unwrap().lamports, 200);
}

#[test]
fn execute_accepts_duplicate_instruction_account_indices() {
    let result = ExecuteBuilder::default()
        .inner_instruction(transfer(&DEFAULT_AUTHORITY, &DEFAULT_AUTHORITY, 1))
        .execute();

    assert_eq!(result.message.instructions()[0].accounts, [0, 0]);
}
