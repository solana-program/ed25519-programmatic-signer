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
    solana_instruction::AccountMeta,
    solana_instruction_error::InstructionError,
    solana_program_error::ProgramError,
    spl_ed25519_programmatic_signer_interface::id,
    spl_programmatic_signer_core::{
        error::Error as NonceError,
        instruction::advance_nonce,
        state::{NONCE_ADVANCE_TAG, Nonce},
    },
    test_case::test_case,
};

pub mod helpers;

#[test_case(0)]
#[test_case(1)]
fn advance_rejects_missing_accounts(account_count: usize) {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let current_nonce = decode_state(&nonce_account).nonce;

    let mut instruction = advance_nonce(
        &id(),
        &nonce_account_address,
        &authority,
        current_nonce,
        Hash::default(),
    );
    instruction.accounts.truncate(account_count);

    mollusk.process_and_validate_instruction(
        &instruction,
        &[
            (authority, Account::default()),
            (nonce_account_address, nonce_account),
        ],
        &[Check::err(ProgramError::NotEnoughAccountKeys)],
    );
}

#[test]
fn advance_rejects_account_owned_by_another_program() {
    AdvanceBuilder::default()
        .nonce_account(
            NonceAccountBuilder::new()
                .owner(Address::new_unique())
                .build(),
        )
        .check(Check::err(ProgramError::IllegalOwner))
        .execute();
}

#[test_case(vec![1; Nonce::LEN.checked_sub(1).unwrap()])]
#[test_case(vec![1; Nonce::LEN.checked_add(1).unwrap()])]
fn advance_rejects_wrong_nonce_account_data_size(malformed_data: Vec<u8>) {
    AdvanceBuilder::default()
        .nonce_account(NonceAccountBuilder::new().data(malformed_data).build())
        .current_nonce(Hash::new_from_array([9; 32]))
        .check(Check::err(NonceError::InvalidNonceAccount.into()))
        .execute();
}

#[test]
fn advance_rejects_uninitialized_nonce_account() {
    AdvanceBuilder::default()
        .nonce_account(NonceAccountBuilder::new().build())
        .check(Check::err(NonceError::InvalidNonceAccount.into()))
        .execute();
}

#[test]
fn advance_rejects_wrong_authority() {
    AdvanceBuilder::default()
        .advance_authority(Address::new_unique())
        .check(Check::err(NonceError::AuthorityMismatch.into()))
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
fn advance_rejects_incorrect_current_nonce() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);

    let wrong_nonce = Hash::new_from_array([9; 32]);
    let result = AdvanceBuilder::default()
        .nonce_account((nonce_account_address, nonce_account.clone()))
        .current_nonce(wrong_nonce)
        .check(Check::err(NonceError::NonceMismatch.into()))
        .execute();

    assert_eq!(
        result.get_account(&nonce_account_address),
        Some(&nonce_account)
    );
}

#[test]
fn advance_rejects_reuse_of_consumed_nonce() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let consumed_nonce = decode_state(&nonce_account).nonce;

    let first = AdvanceBuilder::default()
        .nonce_account((nonce_account_address, nonce_account))
        .execute();
    let advanced = (
        nonce_account_address,
        first.get_account(&nonce_account_address).unwrap().clone(),
    );

    AdvanceBuilder::default()
        .nonce_account(advanced)
        .current_nonce(consumed_nonce)
        .check(Check::err(NonceError::NonceMismatch.into()))
        .execute();
}

#[test]
fn repeated_salt_produces_distinct_successors() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let salt = Hash::new_from_array([7; 32]);

    let first = AdvanceBuilder::default()
        .nonce_account((nonce_account_address, nonce_account))
        .salt(salt)
        .execute();
    let first_account = first.get_account(&nonce_account_address).unwrap().clone();
    let first_nonce = decode_state(&first_account).nonce;

    let second = AdvanceBuilder::default()
        .nonce_account((nonce_account_address, first_account))
        .salt(salt)
        .execute();
    let second_nonce = decode_state(second.get_account(&nonce_account_address).unwrap()).nonce;

    assert_ne!(second_nonce, first_nonce);
}

#[test]
fn advance_rejects_readonly_nonce_account() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let original_nonce_account = nonce_account.clone();
    let current_nonce = decode_state(&nonce_account).nonce;

    let mut instruction = advance_nonce(
        &id(),
        &nonce_account_address,
        &authority,
        current_nonce,
        Hash::default(),
    );
    instruction.accounts[1] = AccountMeta::new_readonly(nonce_account_address, false);

    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            (authority, Account::default()),
            (nonce_account_address, nonce_account),
        ],
        &[Check::instruction_err(
            InstructionError::ReadonlyDataModified,
        )],
    );

    assert_eq!(
        result.get_account(&nonce_account_address),
        Some(&original_nonce_account)
    );
}

#[test]
fn advance_writes_expected_state() {
    let authority = Address::from([2; 32]);
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let initial = decode_state(&nonce_account);
    let old_nonce = initial.nonce;
    let salt = Hash::new_from_array([7; 32]);

    let result = AdvanceBuilder::default()
        .authority(authority)
        .nonce_account((nonce_account_address, nonce_account))
        .salt(salt)
        .execute();

    let state = decode_state(result.get_account(&nonce_account_address).unwrap());
    let program_id = id().to_bytes();
    let nonce_account_bytes = nonce_account_address.to_bytes();
    let old_nonce_bytes = old_nonce.to_bytes();
    let salt_bytes = salt.to_bytes();
    assert_eq!(
        state.nonce,
        solana_sha256_hasher::hashv(&[
            NONCE_ADVANCE_TAG,
            &program_id,
            &nonce_account_bytes,
            &old_nonce_bytes,
            &salt_bytes,
        ])
    );
    assert_eq!(
        state.nonce,
        initial.derive_advance_nonce(&id(), &nonce_account_address, &salt)
    );
    assert_eq!(state.authority, authority);
    assert_eq!(u64::from(state.initialize_slot), 42);
}

#[test]
fn advance_accepts_extra_accounts() {
    let authority = Address::from([2; 32]);
    let mollusk = init_mollusk();
    let (nonce_account_address, nonce_account) = initialize_nonce_account(&mollusk, &authority);
    let current_nonce = decode_state(&nonce_account).nonce;
    let extra_account = Address::new_unique();

    let mut instruction = advance_nonce(
        &id(),
        &nonce_account_address,
        &authority,
        current_nonce,
        Hash::default(),
    );
    instruction
        .accounts
        .push(AccountMeta::new_readonly(extra_account, false));

    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            (authority, Account::default()),
            (nonce_account_address, nonce_account),
            (extra_account, Account::default()),
        ],
        &[Check::success()],
    );

    let advanced = decode_state(result.get_account(&nonce_account_address).unwrap());
    assert_ne!(advanced.nonce, current_nonce);
}
