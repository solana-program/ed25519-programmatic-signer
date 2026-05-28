use {
    crate::helpers::{
        common::{decode_state, init_mollusk},
        durable_signer_account_builder::DurableSignerAccountBuilder,
        initialize_builder::InitializeBuilder,
    },
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    solana_instruction::AccountMeta,
    solana_program_error::ProgramError,
    solana_rent::Rent,
    spl_ed25519_durable_signer_client::instruction::initialize,
    spl_ed25519_durable_signer_interface::state::{
        DurableSignerAccount, INIT_NONCE_DERIVATION_TAG,
    },
};

pub mod helpers;

#[test]
fn initialize_rejects_unknown_instruction_discriminator() {
    let invalid_discriminator = 9;
    InitializeBuilder::default()
        .instruction_data(vec![invalid_discriminator])
        .check(Check::err(ProgramError::InvalidInstructionData))
        .execute();
}

#[test]
fn initialize_rejects_trailing_instruction_data() {
    let mut data_with_trailing_byte =
        initialize(&Address::new_unique(), &Address::new_unique()).data;
    data_with_trailing_byte.push(0);

    InitializeBuilder::default()
        .instruction_data(data_with_trailing_byte)
        .check(Check::err(ProgramError::InvalidInstructionData))
        .execute();
}

#[test]
fn initialize_rejects_missing_accounts() {
    let durable_signer = DurableSignerAccountBuilder::new().build();
    let authority = (Address::new_unique(), Account::default());
    let mut instruction = initialize(&durable_signer.0, &authority.0);
    instruction.accounts.pop(); // drop the SlotHashes account meta

    init_mollusk().process_and_validate_instruction(
        &instruction,
        &[durable_signer, authority],
        &[Check::err(ProgramError::NotEnoughAccountKeys)],
    );
}

#[test]
fn initialize_rejects_account_owned_by_another_program() {
    let wrong_owner = Address::new_unique();
    InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .owner(wrong_owner)
                .build(),
        )
        .check(Check::err(ProgramError::IllegalOwner))
        .execute();
}

#[test]
fn initialize_rejects_wrong_slot_hashes_account() {
    let durable_signer = DurableSignerAccountBuilder::new().build();
    let authority = (Address::new_unique(), Account::default());
    let wrong_slot_hashes = (Address::new_unique(), Account::default());

    let mut instruction = initialize(&durable_signer.0, &authority.0);
    instruction.accounts.pop(); // drop the real SlotHashes meta
    instruction
        .accounts
        .push(AccountMeta::new_readonly(wrong_slot_hashes.0, false));

    init_mollusk().process_and_validate_instruction(
        &instruction,
        &[durable_signer, authority, wrong_slot_hashes],
        &[Check::err(ProgramError::InvalidArgument)],
    );
}

#[test]
fn initialize_rejects_wrong_account_data_size() {
    for wrong_space in [
        DurableSignerAccount::LEN.checked_sub(1).unwrap(),
        DurableSignerAccount::LEN.checked_add(1).unwrap(),
    ] {
        InitializeBuilder::default()
            .durable_signer(
                DurableSignerAccountBuilder::new()
                    .data(vec![0; wrong_space])
                    .build(),
            )
            .check(Check::err(ProgramError::InvalidAccountData))
            .execute();
    }
}

#[test]
fn initialize_rejects_underfunded_account() {
    let underfunded = Rent::default()
        .minimum_balance(DurableSignerAccount::LEN)
        .saturating_sub(1);

    InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .lamports(underfunded)
                .build(),
        )
        .check(Check::err(ProgramError::AccountNotRentExempt))
        .execute();
}

#[test]
fn initialize_rejects_reinitialization() {
    let durable_signer_addr = Address::new_unique();
    let initialized = InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .key(Address::from(&durable_signer_addr))
                .build(),
        )
        .execute()
        .get_account(&durable_signer_addr)
        .unwrap()
        .clone();

    InitializeBuilder::default()
        .durable_signer((durable_signer_addr, initialized))
        .check(Check::err(ProgramError::AccountAlreadyInitialized))
        .execute();
}

#[test]
fn initialize_writes_expected_state() {
    let durable_signer_addr = Address::new_unique();
    let authority_addr = Address::new_unique();
    let result = InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .key(Address::from(&durable_signer_addr))
                .build(),
        )
        .authority_addr(Address::from(&authority_addr))
        .execute();
    let state = decode_state(result.get_account(&durable_signer_addr).unwrap());

    let slot_hash = init_mollusk().sysvars.slot_hashes.first().unwrap().1;
    assert_eq!(
        state.nonce,
        solana_sha256_hasher::hashv(&[
            INIT_NONCE_DERIVATION_TAG,
            durable_signer_addr.as_ref(),
            slot_hash.as_ref(),
        ])
    );
    assert_eq!(state.authority, authority_addr);
}

#[test]
fn initialize_nonce_is_unique_per_durable_signer_account() {
    let first_durable_signer_addr = Address::new_unique();
    let second_durable_signer_addr = Address::new_unique();
    let first = InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .key(Address::from(&first_durable_signer_addr))
                .build(),
        )
        .execute();
    let first_state = decode_state(first.get_account(&first_durable_signer_addr).unwrap());

    let second = InitializeBuilder::default()
        .durable_signer(
            DurableSignerAccountBuilder::new()
                .key(Address::from(&second_durable_signer_addr))
                .build(),
        )
        .execute();
    let second_state = decode_state(second.get_account(&second_durable_signer_addr).unwrap());

    assert_eq!(first_state.authority, second_state.authority);
    assert_ne!(first_state.nonce, second_state.nonce);
}
