use {
    crate::helpers::common::{init_mollusk, initialize_nonce_account},
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_program_error::ProgramError,
    spl_nonce_client::instruction::{advance, withdraw},
    spl_nonce_interface::{error::Error, state::Nonce},
    test_case::test_case,
};

pub mod helpers;

#[test_case(0; "zero")]
#[test_case(99; "above rent minimum")]
#[test_case(100; "exact rent minimum")]
fn partial_withdraw_preserves_state_in_initialization_slot(amount: u64) {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let instruction = withdraw(&authority, &nonce_address, &destination, amount);
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    let (nonce_address, original) = &accounts[1];
    let nonce = result.get_account(nonce_address).unwrap();
    assert_eq!(nonce.data, original.data);
    assert_eq!(nonce.owner, original.owner);
    assert_eq!(
        nonce.lamports,
        original.lamports.checked_sub(amount).unwrap()
    );
    assert_eq!(
        result.get_account(&destination).unwrap().lamports,
        amount.checked_add(1_000_000).unwrap()
    );
}

#[test]
fn withdraw_rejects_one_below_rent_minimum() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let amount = 101;
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let instruction = withdraw(&authority, &nonce_address, &destination, amount);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::AccountNotRentExempt)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_one_lamport_remaining() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let amount = nonce.lamports.checked_sub(1).unwrap();
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let instruction = withdraw(&authority, &nonce_address, &destination, amount);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::AccountNotRentExempt)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test_case(41; "earlier slot")]
#[test_case(42; "initialization slot")]
fn close_rejects_slot_not_after_initialization(slot: u64) {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    mollusk.sysvars.clock.slot = slot;
    let instruction = withdraw(
        &authority,
        &nonce_address,
        &destination,
        accounts[1].1.lamports,
    );
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(Error::CloseSameSlot.into())],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn full_withdraw_closes_account_in_later_slot() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    mollusk.sysvars.clock.slot = 43;
    let balance = accounts[1].1.lamports;
    let instruction = withdraw(&authority, &nonce_address, &destination, balance);
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    let closed = result.get_account(&nonce_address).unwrap();
    assert_eq!(closed.lamports, 0);
    assert!(closed.data.is_empty());
    assert_eq!(closed.owner, Address::default());
    assert_eq!(
        result.get_account(&destination).unwrap().lamports,
        1_000_000 + balance
    );
}

#[test]
fn advance_does_not_bypass_same_slot_close_guard() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let state: Nonce = wincode::deserialize_exact(&accounts[1].1.data).unwrap();
    let advance_instruction = advance(&accounts[0].0, &accounts[1].0, state.nonce, Hash::default());
    let advanced = mollusk.process_and_validate_instruction(
        &advance_instruction,
        &accounts,
        &[Check::success()],
    );
    accounts[1].1 = advanced.get_account(&accounts[1].0).unwrap().clone();
    let instruction = withdraw(
        &authority,
        &nonce_address,
        &destination,
        accounts[1].1.lamports,
    );
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(Error::CloseSameSlot.into())],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn partial_withdraw_requires_authority_signature() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    mollusk.sysvars.clock.slot = 43;
    instruction.accounts[0].is_signer = false;
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn full_withdraw_requires_authority_signature() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, nonce.lamports);
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    mollusk.sysvars.clock.slot = 43;
    instruction.accounts[0].is_signer = false;
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_wrong_authority() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let wrong = Address::new_unique();
    instruction.accounts[0].pubkey = wrong;
    accounts[0].0 = wrong;
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(Error::AuthorityMismatch.into())],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_overdraw() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    let instruction = withdraw(
        &authority,
        &nonce_address,
        &destination,
        accounts[1].1.lamports + 1,
    );
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InsufficientFunds)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_destination_overflow() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    accounts[2].1.lamports = u64::MAX;
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::ArithmeticOverflow)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_self_destination() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    instruction.accounts[2].pubkey = accounts[1].0;
    accounts.pop();
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::InvalidArgument)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_to_authority_is_allowed() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    instruction.accounts[2].pubkey = accounts[0].0;
    accounts.pop();
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);
    assert_eq!(result.get_account(&authority).unwrap().lamports, 100);
}

#[test]
fn withdraw_rejects_wrong_owner() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    accounts[1].1.owner = Address::new_unique();
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::IllegalOwner)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test_case(Nonce::LEN - 1; "too short")]
#[test_case(Nonce::LEN + 1; "too long")]
fn withdraw_rejects_wrong_data_length(len: usize) {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    accounts[1].1.data.resize(len, 0);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(Error::InvalidNonceAccount.into())],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_uninitialized_account() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    accounts[1].1.data.fill(0);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(Error::InvalidNonceAccount.into())],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test_case(1, Error::InvalidNonceAccount.into(); "nonce")]
#[test_case(2, ProgramError::InvalidArgument; "destination")]
fn withdraw_requires_writable_accounts(index: usize, error: ProgramError) {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    instruction.accounts[index].is_writable = false;
    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::err(error)]);
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}

#[test]
fn withdraw_rejects_missing_accounts() {
    let mut mollusk = init_mollusk();
    mollusk.sysvars.clock.slot = 42;
    let authority = Address::new_unique();
    let (nonce_address, mut nonce) = initialize_nonce_account(&mollusk, &authority);
    nonce.lamports = nonce.lamports.checked_add(100).unwrap();
    let destination = Address::new_unique();
    let mut instruction = withdraw(&authority, &nonce_address, &destination, 100);
    let mut accounts = vec![
        (authority, Account::default()),
        (nonce_address, nonce),
        (
            destination,
            Account {
                lamports: 1_000_000,
                ..Account::default()
            },
        ),
    ];
    instruction.accounts.truncate(2);
    accounts.pop();
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[Check::err(ProgramError::NotEnoughAccountKeys)],
    );
    for (address, account) in &accounts {
        assert_eq!(result.get_account(address), Some(account));
    }
}
