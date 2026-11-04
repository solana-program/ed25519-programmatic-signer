use {
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::v1::{self, MessageError},
    spl_ed25519_signer_client::message::{AuthorizationMessageError, authorization_message},
    test_case::test_case,
};

fn instruction_with_accounts(count: usize) -> Instruction {
    Instruction {
        program_id: Address::new_unique(),
        accounts: (0..count)
            .map(|_| AccountMeta::new(Address::new_unique(), false))
            .collect(),
        data: vec![],
    }
}

fn unique_addresses(count: usize) -> Vec<Address> {
    (0..count).map(|_| Address::new_unique()).collect()
}

#[test]
fn accepts_the_signature_and_address_limits() {
    let authorities = unique_addresses(usize::from(v1::MAX_SIGNATURES));
    // The executor program id takes the remaining account key.
    let accounts = usize::from(v1::MAX_ADDRESSES - v1::MAX_SIGNATURES - 1);
    let message =
        authorization_message(&instruction_with_accounts(accounts), &authorities).unwrap();
    assert_eq!(message.account_keys.len(), usize::from(v1::MAX_ADDRESSES));
}

#[test]
fn rejects_too_many_authorities() {
    let authorities = unique_addresses(usize::from(v1::MAX_SIGNATURES) + 1);
    assert_eq!(
        authorization_message(&instruction_with_accounts(0), &authorities),
        Err(AuthorizationMessageError::Invalid(
            MessageError::TooManySignatures
        )),
    );
}

// 300 extra accounts exceed the 256 account keys that compiled u8 indexes can address.
#[test_case(1; "one over the v1 limit")]
#[test_case(300; "beyond u8 indexes")]
fn rejects_too_many_account_keys(excess: usize) {
    // One authority and the executor program id take two account keys.
    let accounts = usize::from(v1::MAX_ADDRESSES - 2).saturating_add(excess);
    let authorities = unique_addresses(1);
    assert_eq!(
        authorization_message(&instruction_with_accounts(accounts), &authorities),
        Err(AuthorizationMessageError::Invalid(
            MessageError::TooManyAddresses
        )),
    );
}

#[test]
fn rejects_no_authorities() {
    assert_eq!(
        authorization_message(&instruction_with_accounts(1), &[]),
        Err(AuthorizationMessageError::Invalid(
            MessageError::ZeroSigners
        )),
    );
}

#[test]
fn rejects_duplicate_authorities() {
    let authority = Address::new_unique();
    assert_eq!(
        authorization_message(&instruction_with_accounts(1), &[authority, authority]),
        Err(AuthorizationMessageError::Invalid(
            MessageError::DuplicateAddresses
        )),
    );
}
