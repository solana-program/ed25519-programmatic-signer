use {
    crate::helpers::{
        advance_builder::AdvanceBuilder,
        common::{decode_state, init_mollusk, initialize_nonce_account},
        nonce_account_builder::NonceAccountBuilder,
    },
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, error::InstructionError},
    solana_program_error::ProgramError,
    spl_nonce_client::instruction::advance,
    spl_nonce_interface::{error::Error as NonceError, state::NONCE_DERIVATION_TAG},
};

pub mod helpers;

fn nonce_error(error: NonceError) -> ProgramError {
    ProgramError::Custom(error as u32)
}

#[test]
fn advance_succeeds_with_program_derivation() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let old_nonce = decode_state(&nonce_account.1).nonce;

    let result = AdvanceBuilder::default()
        .nonce_account(nonce_account.clone())
        .execute();

    let state = decode_state(result.get_account(&nonce_account.0).unwrap());
    let slot_hash = mollusk.sysvars.slot_hashes.first().unwrap().1;
    let nonce_account_address = nonce_account.0.to_bytes();
    let old_nonce = old_nonce.to_bytes();
    let slot_hash = slot_hash.to_bytes();
    assert_eq!(
        state.nonce,
        solana_sha256_hasher::hashv(&[
            NONCE_DERIVATION_TAG,
            &nonce_account_address,
            &old_nonce,
            &slot_hash,
        ])
    );
    assert_eq!(state.authority, authority);
}

#[test]
fn advance_rejects_reuse_of_consumed_nonce() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let consumed_nonce = decode_state(&nonce_account.1).nonce;

    let first = AdvanceBuilder::default()
        .nonce_account(nonce_account.clone())
        .execute();
    let advanced = (
        nonce_account.0,
        first.get_account(&nonce_account.0).unwrap().clone(),
    );

    AdvanceBuilder::default()
        .nonce_account(advanced)
        .current_nonce(consumed_nonce)
        .check(Check::err(nonce_error(NonceError::NonceMismatch)))
        .execute();
}

#[test]
fn advance_rejects_stale_current_nonce() {
    AdvanceBuilder::default()
        .current_nonce(Hash::new_from_array([9; 32]))
        .check(Check::err(nonce_error(NonceError::NonceMismatch)))
        .execute();
}

#[test]
fn advance_requires_authority_signature() {
    AdvanceBuilder::default()
        .authority_not_signer()
        .check(Check::err(ProgramError::MissingRequiredSignature))
        .execute();
}

#[test]
fn advance_rejects_wrong_authority() {
    AdvanceBuilder::default()
        .advance_authority(Address::new_unique())
        .check(Check::err(nonce_error(NonceError::AuthorityMismatch)))
        .execute();
}

#[test]
fn advance_rejects_account_owned_by_another_program() {
    AdvanceBuilder::default()
        .nonce_account(
            NonceAccountBuilder::new()
                .key(Address::new_unique())
                .owner(Address::new_unique())
                .build(),
        )
        .current_nonce(Hash::new_from_array([9; 32]))
        .check(Check::err(ProgramError::IllegalOwner))
        .execute();
}

#[test]
fn advance_rejects_readonly_nonce_account() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let current_nonce = decode_state(&nonce_account.1).nonce;

    let mut instruction = advance(&nonce_account.0, &authority, current_nonce);
    instruction.accounts[0] = AccountMeta::new_readonly(nonce_account.0, false);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[
            nonce_account,
            (authority, Account::default()),
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[Check::instruction_err(
            InstructionError::ReadonlyDataModified,
        )],
    );
}

#[test]
fn advance_rejects_malformed_nonce_account() {
    AdvanceBuilder::default()
        .nonce_account(
            NonceAccountBuilder::new()
                .key(Address::new_unique())
                .data(vec![0; 1])
                .build(),
        )
        .current_nonce(Hash::new_from_array([9; 32]))
        .check(Check::err(nonce_error(NonceError::InvalidNonceAccount)))
        .execute();
}

#[test]
fn advance_rejects_uninitialized_nonce_account() {
    AdvanceBuilder::default()
        .nonce_account(
            NonceAccountBuilder::new()
                .key(Address::new_unique())
                .build(),
        )
        .advance_authority(Address::default())
        .check(Check::err(nonce_error(NonceError::InvalidNonceAccount)))
        .execute();
}

#[test]
fn advance_rejects_wrong_slot_hashes_account() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let current_nonce = decode_state(&nonce_account.1).nonce;

    let wrong_slot_hashes = Address::new_unique();
    let mut instruction = advance(&nonce_account.0, &authority, current_nonce);
    instruction.accounts[2] = AccountMeta::new_readonly(wrong_slot_hashes, false);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[
            nonce_account,
            (authority, Account::default()),
            (wrong_slot_hashes, Account::default()),
        ],
        &[Check::err(ProgramError::InvalidArgument)],
    );
}

#[test]
fn advance_rejects_missing_accounts() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let nonce_account = initialize_nonce_account(&mollusk, &authority);
    let current_nonce = decode_state(&nonce_account.1).nonce;

    let mut instruction = advance(&nonce_account.0, &authority, current_nonce);
    instruction.accounts.truncate(1);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[nonce_account],
        &[Check::err(ProgramError::NotEnoughAccountKeys)],
    );
}
