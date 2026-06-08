use {
    crate::helpers::{
        common::{
            advance_slot_hash, decode_state, init_mollusk, initialize_durable_signer,
            signer_account, system_transfer_instruction, writable_system_account,
        },
        submit_builder::{
            SignedV1Message, SubmitBuilder, compiled_transfer_instruction, empty_v1_message,
            submit_instruction_for_v1_message, v1_message_bytes, wrapped_address, wrapped_hash,
            wrapped_signature,
        },
    },
    mollusk_svm::result::Check,
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_message::{
        Message, MessageHeader,
        compiled_instruction::CompiledInstruction,
        v1::{Message as MessageV1, TransactionConfig},
    },
    solana_program_error::ProgramError,
    solana_signer::Signer,
    solana_transaction::{VersionedMessage, versioned::VersionedTransaction},
    spl_ed25519_durable_signer_interface::{
        error::DurableSignerError, instruction::DurableSignerInstruction, pda::DurableSignerPda,
    },
};

pub mod helpers;

fn address(tag: u8) -> Address {
    let mut bytes = [0; 32];
    bytes[0] = tag;
    Address::new_from_array(bytes)
}

fn durable_signer_error(error: DurableSignerError) -> ProgramError {
    ProgramError::Custom(error as u32)
}

#[test]
fn submit_executes_authorized_transfer_and_advances_nonce() {
    let authority = Keypair::new();
    let recipient = address(0x10);
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &authority.pubkey(),
    );
    let mut mollusk = init_mollusk();
    advance_slot_hash(&mut mollusk, 0xa1);
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());
    let initial_nonce = decode_state(&durable_signer.1).nonce;

    advance_slot_hash(&mut mollusk, 0xa2);
    let result = SubmitBuilder::new(authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .durable_signer_pda_lamports(5_000)
        .inner_instruction(system_transfer_instruction(authority_pda, recipient, 1_000))
        .execute();

    assert_eq!(result.account(&recipient).unwrap().lamports, 1_000);
    assert_ne!(decode_state(&result.durable_signer.1).nonce, initial_nonce);
}

#[test]
fn submit_rejects_replay_after_nonce_advances() {
    let authority = Keypair::new();
    let recipient = address(0x20);
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &authority.pubkey(),
    );
    let mut mollusk = init_mollusk();
    advance_slot_hash(&mut mollusk, 0xb1);
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());

    advance_slot_hash(&mut mollusk, 0xb2);
    let first = SubmitBuilder::new(authority.insecure_clone())
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .durable_signer_pda_lamports(5_000)
        .inner_instruction(system_transfer_instruction(authority_pda, recipient, 100))
        .execute();

    let replay_pda_account = first.account(&authority_pda).unwrap().clone();
    let replay_recipient = first.account(&recipient).unwrap().clone();
    SubmitBuilder::new(authority)
        .durable_signer(first.durable_signer)
        .submit_instruction(first.submit_instruction)
        .account(authority_pda, replay_pda_account)
        .account(recipient, replay_recipient)
        .check(Check::err(durable_signer_error(
            DurableSignerError::NonceMismatch,
        )))
        .execute();
}

#[test]
fn submit_rejects_stale_lifetime_specifier() {
    let authority = Keypair::new();
    SubmitBuilder::new(authority)
        .lifetime_specifier(Hash::new_from_array([0x44; 32]))
        .check(Check::err(durable_signer_error(
            DurableSignerError::NonceMismatch,
        )))
        .execute();
}

#[test]
fn submit_rejects_neighboring_outer_instruction() {
    let authority = Keypair::new();
    let payer = Keypair::new();
    let recipient = address(0x30);
    let neighbor = system_transfer_instruction(payer.pubkey(), recipient, 1);

    SubmitBuilder::new(authority)
        .pre_outer_instruction(neighbor)
        .account(payer.pubkey(), signer_account(10))
        .account(recipient, writable_system_account(0))
        .check(Check::err(durable_signer_error(
            DurableSignerError::OuterTxMustContainOnlySubmit,
        )))
        .execute();
}

#[test]
fn submit_rejects_missing_wrapped_signature() {
    let authority = Keypair::new();
    SubmitBuilder::new(authority)
        .zero_signature_at(0)
        .check(Check::err(durable_signer_error(
            DurableSignerError::MissingAuthorization,
        )))
        .execute();
}

#[test]
fn submit_rejects_raw_keypair_required_signer() {
    let authority = Keypair::new();
    let mollusk = init_mollusk();
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());
    let nonce = decode_state(&durable_signer.1).nonce;
    let message = empty_v1_message(nonce, authority.pubkey());
    let message_bytes = v1_message_bytes(&message);
    let signed = SignedV1Message {
        signatures: vec![wrapped_signature(
            authority.try_sign_message(&message_bytes).unwrap(),
        )],
        authorities: vec![authority.pubkey()],
        message_bytes,
    };
    let submit = submit_instruction_for_v1_message(
        spl_ed25519_durable_signer_interface::id(),
        durable_signer.0,
        &message,
        &signed,
    );

    SubmitBuilder::new(authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .submit_instruction(submit)
        .check(Check::err(durable_signer_error(
            DurableSignerError::IncorrectAuthorityPda,
        )))
        .execute();
}

#[test]
fn submit_rejects_wrapped_transaction_config() {
    let authority = Keypair::new();
    let mollusk = init_mollusk();
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());
    let nonce = decode_state(&durable_signer.1).nonce;
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &authority.pubkey(),
    );
    let message = MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        config: TransactionConfig {
            compute_unit_limit: Some(100_000),
            ..TransactionConfig::empty()
        },
        lifetime_specifier: wrapped_hash(nonce),
        account_keys: vec![wrapped_address(authority_pda)],
        instructions: vec![],
    };
    let message_bytes = v1_message_bytes(&message);
    let signed = SignedV1Message {
        signatures: vec![wrapped_signature(
            authority.try_sign_message(&message_bytes).unwrap(),
        )],
        authorities: vec![authority.pubkey()],
        message_bytes,
    };
    let submit = submit_instruction_for_v1_message(
        spl_ed25519_durable_signer_interface::id(),
        durable_signer.0,
        &message,
        &signed,
    );

    SubmitBuilder::new(authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .submit_instruction(submit)
        .check(Check::err(durable_signer_error(
            DurableSignerError::InvalidWrappedTransaction,
        )))
        .execute();
}

#[test]
fn submit_accepts_multiple_wrapped_signers() {
    let authority = Keypair::new();
    let extra_authority = Keypair::new();
    let extra_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &extra_authority.pubkey(),
    );
    let recipient = address(0x40);

    let result = SubmitBuilder::new(authority)
        .additional_authority(extra_authority)
        .account(extra_pda, writable_system_account(5_000))
        .inner_instruction(system_transfer_instruction(extra_pda, recipient, 777))
        .execute();

    assert_eq!(result.account(&recipient).unwrap().lamports, 777);
}

#[test]
fn submit_rejects_wrong_authority_in_payload() {
    let real_authority = Keypair::new();
    let attacker = Keypair::new();
    let mollusk = init_mollusk();
    let durable_signer = initialize_durable_signer(&mollusk, &real_authority.pubkey());
    let nonce = decode_state(&durable_signer.1).nonce;
    let attacker_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &attacker.pubkey(),
    );
    let message = empty_v1_message(nonce, attacker_pda);
    let message_bytes = v1_message_bytes(&message);
    let signed = SignedV1Message {
        signatures: vec![wrapped_signature(
            attacker.try_sign_message(&message_bytes).unwrap(),
        )],
        authorities: vec![attacker.pubkey()],
        message_bytes,
    };
    let submit = submit_instruction_for_v1_message(
        spl_ed25519_durable_signer_interface::id(),
        durable_signer.0,
        &message,
        &signed,
    );

    SubmitBuilder::new(real_authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .submit_instruction(submit)
        .account(attacker.pubkey(), signer_account(0))
        .check(Check::err(durable_signer_error(
            DurableSignerError::AuthorityMismatch,
        )))
        .execute();
}

#[test]
fn submit_can_batch_multiple_inner_instructions() {
    let authority = Keypair::new();
    let recipient_a = address(0x50);
    let recipient_b = address(0x51);
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &authority.pubkey(),
    );

    let result = SubmitBuilder::new(authority)
        .durable_signer_pda_lamports(10_000)
        .inner_instruction(system_transfer_instruction(authority_pda, recipient_a, 123))
        .inner_instruction(system_transfer_instruction(authority_pda, recipient_b, 456))
        .execute();

    assert_eq!(result.account(&recipient_a).unwrap().lamports, 123);
    assert_eq!(result.account(&recipient_b).unwrap().lamports, 456);
}

#[test]
fn submit_rejects_non_pda_signer_even_when_authority_is_not_first() {
    let authority = Keypair::new();
    let other = Keypair::new();
    let mollusk = init_mollusk();
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());
    let nonce = decode_state(&durable_signer.1).nonce;
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &authority.pubkey(),
    );
    let recipient = address(0x60);
    let system_program = solana_system_interface::program::id();
    let message = MessageV1 {
        header: MessageHeader {
            num_required_signatures: 2,
            num_readonly_signed_accounts: 1,
            num_readonly_unsigned_accounts: 1,
        },
        config: TransactionConfig::empty(),
        lifetime_specifier: wrapped_hash(nonce),
        account_keys: vec![
            wrapped_address(other.pubkey()),
            wrapped_address(authority_pda),
            wrapped_address(recipient),
            wrapped_address(system_program),
        ],
        instructions: vec![compiled_transfer_instruction(1, 2, 3, 1)],
    };
    let message_bytes = v1_message_bytes(&message);
    let signed = SignedV1Message {
        signatures: vec![
            wrapped_signature(other.try_sign_message(&message_bytes).unwrap()),
            wrapped_signature(authority.try_sign_message(&message_bytes).unwrap()),
        ],
        authorities: vec![other.pubkey(), authority.pubkey()],
        message_bytes,
    };
    let submit = submit_instruction_for_v1_message(
        spl_ed25519_durable_signer_interface::id(),
        durable_signer.0,
        &message,
        &signed,
    );

    SubmitBuilder::new(authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .submit_instruction(submit)
        .account(other.pubkey(), signer_account(0))
        .check(Check::err(durable_signer_error(
            DurableSignerError::IncorrectAuthorityPda,
        )))
        .execute();
}

#[test]
fn submit_rejects_wrapped_submit_cpi() {
    let authority = Keypair::new();
    let mollusk = init_mollusk();
    let durable_signer = initialize_durable_signer(&mollusk, &authority.pubkey());
    let nonce = decode_state(&durable_signer.1).nonce;
    let program_id = spl_ed25519_durable_signer_interface::id();
    let authority_pda = DurableSignerPda::derive_address(&program_id, &authority.pubkey());
    let inner_transaction = VersionedTransaction {
        signatures: vec![],
        message: VersionedMessage::Legacy(Message::default()),
    };
    let inner_submit_data =
        wincode::serialize(&DurableSignerInstruction::Submit(inner_transaction)).unwrap();
    let message = MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 3,
        },
        config: TransactionConfig::empty(),
        lifetime_specifier: wrapped_hash(nonce),
        account_keys: vec![
            wrapped_address(authority_pda),
            wrapped_address(durable_signer.0),
            wrapped_address(solana_sdk_ids::sysvar::slot_hashes::ID),
            wrapped_address(solana_sdk_ids::sysvar::instructions::ID),
            wrapped_address(program_id),
        ],
        instructions: vec![CompiledInstruction {
            program_id_index: 4,
            accounts: vec![1, 2, 3],
            data: inner_submit_data,
        }],
    };
    let message_bytes = v1_message_bytes(&message);
    let signed = SignedV1Message {
        signatures: vec![wrapped_signature(
            authority.try_sign_message(&message_bytes).unwrap(),
        )],
        authorities: vec![authority.pubkey()],
        message_bytes,
    };
    let submit = submit_instruction_for_v1_message(program_id, durable_signer.0, &message, &signed);

    SubmitBuilder::new(authority)
        .mollusk(mollusk)
        .durable_signer(durable_signer)
        .submit_instruction(submit)
        .account(
            program_id,
            mollusk_svm::program::create_program_account_loader_v3(&program_id),
        )
        .check(Check::err(durable_signer_error(
            DurableSignerError::OuterTxMustContainOnlySubmit,
        )))
        .execute();
}
