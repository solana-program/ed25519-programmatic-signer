//! End-to-end Falcon-512 coverage for the Falcon program variant.
//!
//! These tests intentionally exercise the concrete Falcon wire format:
//!
//! - `Initialize(FalconInitialize)` stores a hash-derived authority id plus a
//!   prepared public key in the durable signer account.
//! - `Submit(FalconSubmit)` carries the wrapped message and Falcon signatures
//!   directly in instruction data. No per-message authorization is pre-written
//!   into account data, so submit is atomic.
//!
//! Run with a Falcon SBF artifact:
//! ```text
//! cargo build-sbf --manifest-path program/Cargo.toml -- --features falcon
//! SBF_OUT_DIR=/absolute/path/to/target/deploy \
//!   cargo test -p spl-ed25519-durable-signer-program --features falcon --test test_falcon_submit
//! ```

use {
    crate::helpers::{
        common::{init_mollusk, writable_system_account},
        durable_signer_account_builder::DurableSignerAccountBuilder,
        submit_builder::{
            compiled_transfer_instruction, is_v1_maybe_writable, unwrapped_address,
            v1_message_bytes, wrapped_address, wrapped_hash,
        },
    },
    mollusk_svm::result::Check,
    pqcrypto_falcon::falcon512,
    pqcrypto_traits::sign::{DetachedSignature, PublicKey},
    solana_account::Account,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::{
        MessageHeader,
        v1::{Message as MessageV1, TransactionConfig},
    },
    solana_program_error::ProgramError,
    solana_transaction::VersionedMessage,
    spl_ed25519_durable_signer_interface::{
        error::DurableSignerError,
        instruction::{
            FALCON512_PUBLIC_KEY_LEN, FalconDurableSignerInstruction, FalconInitialize,
            FalconSignature, FalconSubmit,
        },
        pda::DurableSignerPda,
        state::{FalconDurableSignerAccount, INIT_NONCE_DERIVATION_TAG, falcon_authority_id},
    },
};

pub mod helpers;

fn durable_signer_error(error: DurableSignerError) -> ProgramError {
    ProgramError::Custom(error as u32)
}

fn falcon_public_key(public_key: &impl PublicKey) -> [u8; FALCON512_PUBLIC_KEY_LEN] {
    public_key.as_bytes().try_into().unwrap()
}

fn padded_signature(raw: &[u8]) -> FalconSignature {
    FalconSignature::try_from_compressed(raw).unwrap()
}

fn falcon_initialize_instruction(
    durable_signer: Address,
    public_key: [u8; FALCON512_PUBLIC_KEY_LEN],
) -> Instruction {
    Instruction {
        program_id: spl_ed25519_durable_signer_interface::id(),
        accounts: vec![
            AccountMeta::new(durable_signer, false),
            AccountMeta::new_readonly(solana_sdk_ids::sysvar::slot_hashes::ID, false),
        ],
        data: wincode::serialize(&FalconDurableSignerInstruction::Initialize(
            FalconInitialize { public_key },
        ))
        .unwrap(),
    }
}

fn initialize_falcon_durable_signer(public_key: [u8; FALCON512_PUBLIC_KEY_LEN]) -> FalconFixture {
    let mollusk = init_mollusk();
    let durable_signer = DurableSignerAccountBuilder::new()
        .key(Address::new_unique())
        .data(vec![0; FalconDurableSignerAccount::LEN])
        .build();
    let instruction = falcon_initialize_instruction(durable_signer.0, public_key);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            durable_signer.clone(),
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[Check::success()],
    );
    let durable_signer = (
        durable_signer.0,
        result.get_account(&durable_signer.0).unwrap().clone(),
    );
    let state: FalconDurableSignerAccount =
        wincode::deserialize_exact(&durable_signer.1.data).unwrap();

    FalconFixture {
        mollusk,
        durable_signer,
        state,
    }
}

struct FalconFixture {
    mollusk: mollusk_svm::Mollusk,
    durable_signer: (Address, Account),
    state: FalconDurableSignerAccount,
}

fn falcon_submit_instruction(
    durable_signer: Address,
    message: &MessageV1,
    signatures: Vec<FalconSignature>,
) -> Instruction {
    let submit = FalconSubmit {
        signatures,
        message: VersionedMessage::V1(message.clone()),
    };
    let mut accounts = Vec::with_capacity(3usize.saturating_add(message.account_keys.len()));
    accounts.push(AccountMeta::new(durable_signer, false));
    accounts.push(AccountMeta::new_readonly(
        solana_sdk_ids::sysvar::slot_hashes::ID,
        false,
    ));
    accounts.push(AccountMeta::new_readonly(
        solana_sdk_ids::sysvar::instructions::ID,
        false,
    ));
    for (index, key) in message.account_keys.iter().enumerate() {
        accounts.push(if is_v1_maybe_writable(message, index) {
            AccountMeta::new(unwrapped_address(*key), false)
        } else {
            AccountMeta::new_readonly(unwrapped_address(*key), false)
        });
    }

    Instruction {
        program_id: spl_ed25519_durable_signer_interface::id(),
        accounts,
        data: wincode::serialize(&FalconDurableSignerInstruction::Submit(submit)).unwrap(),
    }
}

fn falcon_message(
    nonce: solana_hash::Hash,
    authority_pda: Address,
    recipient: Address,
) -> MessageV1 {
    MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        config: TransactionConfig::empty(),
        lifetime_specifier: wrapped_hash(nonce),
        account_keys: vec![
            wrapped_address(authority_pda),
            wrapped_address(recipient),
            wrapped_address(solana_system_interface::program::id()),
        ],
        instructions: vec![compiled_transfer_instruction(0, 1, 2, 1_000)],
    }
}

#[test]
fn falcon_initialize_stores_bound_authority_and_prepared_key() {
    let (public_key, _) = falcon512::keypair();
    let public_key = falcon_public_key(&public_key);
    let fixture = initialize_falcon_durable_signer(public_key);

    let slot_hash = init_mollusk().sysvars.slot_hashes.first().unwrap().1;
    assert_eq!(
        fixture.state.nonce,
        solana_sha256_hasher::hashv(&[
            INIT_NONCE_DERIVATION_TAG,
            fixture.durable_signer.0.as_ref(),
            slot_hash.as_ref(),
        ])
    );
    assert_eq!(fixture.state.authority.id, falcon_authority_id(&public_key));
    assert_ne!(
        fixture.state.authority.prepared_public_key,
        [0; spl_ed25519_durable_signer_interface::state::FALCON512_PREPARED_PUBLIC_KEY_LEN]
    );
}

#[test]
fn falcon_initialize_rejects_invalid_public_key() {
    let mollusk = init_mollusk();
    let durable_signer = DurableSignerAccountBuilder::new()
        .key(Address::new_unique())
        .data(vec![0; FalconDurableSignerAccount::LEN])
        .build();
    let instruction =
        falcon_initialize_instruction(durable_signer.0, [0; FALCON512_PUBLIC_KEY_LEN]);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[
            durable_signer,
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[Check::err(ProgramError::InvalidInstructionData)],
    );
}

#[test]
fn falcon_initialize_rejects_wrong_account_size() {
    let (public_key, _) = falcon512::keypair();
    let public_key = falcon_public_key(&public_key);
    let mollusk = init_mollusk();
    let durable_signer = DurableSignerAccountBuilder::new()
        .key(Address::new_unique())
        .data(vec![
            0;
            FalconDurableSignerAccount::LEN.checked_sub(1).unwrap()
        ])
        .build();
    let instruction = falcon_initialize_instruction(durable_signer.0, public_key);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[
            durable_signer,
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[Check::err(ProgramError::InvalidAccountData)],
    );
}

#[test]
fn falcon_submit_executes_authorized_transfer_and_advances_nonce() {
    let (public_key, secret_key) = falcon512::keypair();
    let public_key = falcon_public_key(&public_key);
    let fixture = initialize_falcon_durable_signer(public_key);
    let recipient = Address::new_unique();
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &fixture.state.authority.id,
    );
    let message = falcon_message(fixture.state.nonce, authority_pda, recipient);
    let message_bytes = v1_message_bytes(&message);
    let signature =
        padded_signature(falcon512::detached_sign(&message_bytes, &secret_key).as_bytes());
    let submit = falcon_submit_instruction(fixture.durable_signer.0, &message, vec![signature]);

    let result = fixture
        .mollusk
        .process_and_validate_transaction_instructions(
            &[submit],
            &[
                fixture.durable_signer.clone(),
                fixture
                    .mollusk
                    .sysvars
                    .keyed_account_for_slot_hashes_sysvar(),
                (authority_pda, writable_system_account(5_000)),
                (recipient, writable_system_account(0)),
                (
                    solana_system_interface::program::id(),
                    mollusk_svm::program::keyed_account_for_system_program().1,
                ),
            ],
            &[Check::success()],
        );

    assert_eq!(result.get_account(&recipient).unwrap().lamports, 1_000);
    let final_state: FalconDurableSignerAccount =
        wincode::deserialize_exact(&result.get_account(&fixture.durable_signer.0).unwrap().data)
            .unwrap();
    assert_ne!(final_state.nonce, fixture.state.nonce);
}

#[test]
fn falcon_submit_rejects_signature_count_mismatch() {
    let (public_key, _) = falcon512::keypair();
    let fixture = initialize_falcon_durable_signer(falcon_public_key(&public_key));
    let recipient = Address::new_unique();
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &fixture.state.authority.id,
    );
    let message = falcon_message(fixture.state.nonce, authority_pda, recipient);
    let submit = falcon_submit_instruction(fixture.durable_signer.0, &message, vec![]);

    fixture
        .mollusk
        .process_and_validate_transaction_instructions(
            &[submit],
            &[
                fixture.durable_signer,
                fixture
                    .mollusk
                    .sysvars
                    .keyed_account_for_slot_hashes_sysvar(),
                (authority_pda, writable_system_account(5_000)),
                (recipient, writable_system_account(0)),
                (
                    solana_system_interface::program::id(),
                    mollusk_svm::program::keyed_account_for_system_program().1,
                ),
            ],
            &[Check::err(durable_signer_error(
                DurableSignerError::InvalidWrappedTransaction,
            ))],
        );
}

#[test]
fn falcon_submit_rejects_tampered_signature() {
    let (public_key, secret_key) = falcon512::keypair();
    let fixture = initialize_falcon_durable_signer(falcon_public_key(&public_key));
    let recipient = Address::new_unique();
    let authority_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &fixture.state.authority.id,
    );
    let message = falcon_message(fixture.state.nonce, authority_pda, recipient);
    let message_bytes = v1_message_bytes(&message);
    let mut signature =
        padded_signature(falcon512::detached_sign(&message_bytes, &secret_key).as_bytes());
    signature.bytes[10] ^= 0x01;
    let submit = falcon_submit_instruction(fixture.durable_signer.0, &message, vec![signature]);

    fixture
        .mollusk
        .process_and_validate_transaction_instructions(
            &[submit],
            &[
                fixture.durable_signer,
                fixture
                    .mollusk
                    .sysvars
                    .keyed_account_for_slot_hashes_sysvar(),
                (authority_pda, writable_system_account(5_000)),
                (recipient, writable_system_account(0)),
                (
                    solana_system_interface::program::id(),
                    mollusk_svm::program::keyed_account_for_system_program().1,
                ),
            ],
            &[Check::err(durable_signer_error(
                DurableSignerError::MissingAuthorization,
            ))],
        );
}

#[test]
fn falcon_submit_rejects_wrong_required_signer_pda() {
    let (public_key, secret_key) = falcon512::keypair();
    let fixture = initialize_falcon_durable_signer(falcon_public_key(&public_key));
    let recipient = Address::new_unique();
    let wrong_pda = DurableSignerPda::derive_address(
        &spl_ed25519_durable_signer_interface::id(),
        &Address::new_unique(),
    );
    let message = falcon_message(fixture.state.nonce, wrong_pda, recipient);
    let message_bytes = v1_message_bytes(&message);
    let signature =
        padded_signature(falcon512::detached_sign(&message_bytes, &secret_key).as_bytes());
    let submit = falcon_submit_instruction(fixture.durable_signer.0, &message, vec![signature]);

    fixture
        .mollusk
        .process_and_validate_transaction_instructions(
            &[submit],
            &[
                fixture.durable_signer,
                fixture
                    .mollusk
                    .sysvars
                    .keyed_account_for_slot_hashes_sysvar(),
                (wrong_pda, writable_system_account(5_000)),
                (recipient, writable_system_account(0)),
                (
                    solana_system_interface::program::id(),
                    mollusk_svm::program::keyed_account_for_system_program().1,
                ),
            ],
            &[Check::err(durable_signer_error(
                DurableSignerError::IncorrectAuthorityPda,
            ))],
        );
}
