use {
    crate::helpers::{
        common::init_mollusk,
        submit_builder::{DEFAULT_PDA_LAMPORTS, DEFAULT_TRANSFER_LAMPORTS, SubmitBuilder},
    },
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address,
    solana_instruction::error::InstructionError,
    solana_keypair::Keypair,
    solana_program_error::ProgramError,
    solana_signature::Signature,
    solana_signer::Signer as _,
    solana_system_interface::instruction::{create_account, transfer},
    spl_ed25519_signer_client::instruction::submit,
    spl_ed25519_signer_interface::{
        error::Error,
        instruction::{SubmitEnvelope, SubmitPayload},
        pda::ProgrammaticSigner,
    },
};

pub mod helpers;

#[test]
fn submit_rejects_empty_signatures() {
    SubmitBuilder::default()
        .signatures(vec![])
        .check_err(Error::NoSignatures)
        .execute();
}

#[test]
fn submit_rejects_wrong_signer_program_id() {
    let wrong_program = Address::new_unique();
    SubmitBuilder::default()
        .signer_program_id(wrong_program)
        .check_err(Error::SignerProgramMismatch)
        .execute();
}

#[test]
fn submit_rejects_more_signatures_than_accounts() {
    // Two signatures in the envelope but only one authority account, so the authority
    // prefix can't be split off (`split_at_checked` returns `None`).
    SubmitBuilder::default()
        .signatures(vec![Signature::from([7; 64]); 2])
        .authority_accounts_only()
        .check_err(ProgramError::NotEnoughAccountKeys)
        .execute();
}

#[test]
fn submit_rejects_missing_accounts() {
    // The executor account (and everything after) is missing
    SubmitBuilder::default()
        .authority_accounts_only()
        .check_err(ProgramError::NotEnoughAccountKeys)
        .execute();
}

#[test]
fn submit_rejects_executor_program_mismatch() {
    // The payload specifies a different executor than the account passed
    let wrong_program = Address::new_unique();
    SubmitBuilder::default()
        .executor_program_id(wrong_program)
        .check_err(Error::ExecutorMismatch)
        .execute();
}

#[test]
fn submit_rejects_invalid_signature() {
    // Garbage bytes signature
    let wrong_signature = Signature::from([7; 64]);
    SubmitBuilder::default()
        .signatures(vec![wrong_signature])
        .check_err(Error::InvalidSignature)
        .execute();
}

#[test]
fn submit_rejects_signature_from_wrong_authority() {
    // A valid signature over the payload, but not by the authority account it is paired with
    let wrong_signer = Keypair::new();
    SubmitBuilder::default()
        .signed_by(0, wrong_signer)
        .check_err(Error::InvalidSignature)
        .execute();
}

#[test]
fn submit_rejects_tampered_instruction_data() {
    // Submits executor data the authority never signed
    let random_data = vec![0xFF];
    SubmitBuilder::default()
        .unsigned_executor_data(random_data)
        .check_err(Error::InvalidSignature)
        .execute();
}

#[test]
fn submit_verifies_every_signature() {
    // The first signature is valid, but the second slot is signed by the wrong key,
    // which fails the whole submit.
    SubmitBuilder::default()
        .additional_authority(Keypair::new())
        .signed_by(1, Keypair::new())
        .check_err(Error::InvalidSignature)
        .execute();
}

#[test]
fn submit_rejects_non_executable_executor() {
    let fake_executor = Address::new_unique();
    SubmitBuilder::default()
        .executor(
            fake_executor,
            Account {
                lamports: 1,
                ..Account::default()
            },
        )
        .check(Check::instruction_err(
            InstructionError::UnsupportedProgramId,
        ))
        .execute();
}

#[test]
fn submit_does_not_promote_unrelated_accounts() {
    // The transfer spends from an account that is no authority's programmatic signer.
    // The signer program forwards it unsigned, so the system program rejects the transfer.
    let authority = Keypair::new();
    let unrelated = Address::new_unique();
    let recipient = Address::new_unique();
    let mut executor_instruction = transfer(&unrelated, &recipient, 0);
    // `unrelated` has no key to sign the outer transaction with.
    executor_instruction.accounts[0].is_signer = false;
    let payload = SubmitPayload {
        signer_program_id: spl_ed25519_signer_interface::id(),
        executor_program_id: executor_instruction.program_id,
        executor_instruction_data: executor_instruction.data.clone(),
    };
    let envelope = SubmitEnvelope {
        signatures: vec![authority.sign_message(&payload.signing_bytes().unwrap())],
        payload,
    };
    let instruction = submit(
        envelope,
        &[authority.pubkey()],
        &executor_instruction.accounts,
    );

    init_mollusk().process_and_validate_instruction(
        &instruction,
        &[
            (authority.pubkey(), Account::default()),
            mollusk_svm::program::keyed_account_for_system_program(),
            (unrelated, Account::default()),
            (recipient, Account::default()),
        ],
        &[Check::err(ProgramError::MissingRequiredSignature)],
    );
}

#[test]
fn submit_promotes_programmatic_signer() {
    let result = SubmitBuilder::default().execute();

    assert_eq!(
        result.account(&result.recipient).unwrap().lamports,
        DEFAULT_TRANSFER_LAMPORTS
    );
    assert_eq!(
        result
            .account(&result.programmatic_signer)
            .unwrap()
            .lamports,
        DEFAULT_PDA_LAMPORTS - DEFAULT_TRANSFER_LAMPORTS
    );
}

#[test]
fn submit_promotes_multiple_authorities() {
    // One executor instruction that requires both programmatic signers to sign:
    // `create_account` needs the funder (first PDA) and the new account (second PDA).
    // Success proves both authorities were promoted in the same submit.
    let first_authority = Keypair::new();
    let second_authority = Keypair::new();
    let first_programmatic_signer = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_interface::id(),
        &first_authority.pubkey(),
    );
    let second_programmatic_signer = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_interface::id(),
        &second_authority.pubkey(),
    );
    let executor_instruction = create_account(
        &first_programmatic_signer,
        &second_programmatic_signer,
        DEFAULT_TRANSFER_LAMPORTS,
        0,
        &solana_system_interface::program::id(),
    );

    let instruction = spl_ed25519_signer_client::signing::sign_and_submit(
        &executor_instruction,
        &[&first_authority, &second_authority],
    )
    .unwrap();

    // Mollusk does not verify signatures, so an outer meta with a fabricated signer flag
    // would forward privilege without promotion and trivially pass this test.
    assert!(instruction.accounts.iter().all(|meta| !meta.is_signer));

    let result = init_mollusk().process_and_validate_instruction(
        &instruction,
        &[
            (first_authority.pubkey(), Account::default()),
            (second_authority.pubkey(), Account::default()),
            mollusk_svm::program::keyed_account_for_system_program(),
            (
                first_programmatic_signer,
                Account {
                    lamports: DEFAULT_PDA_LAMPORTS,
                    ..Account::default()
                },
            ),
            (second_programmatic_signer, Account::default()),
        ],
        &[Check::success()],
    );

    assert_eq!(
        result
            .get_account(&second_programmatic_signer)
            .unwrap()
            .lamports,
        DEFAULT_TRANSFER_LAMPORTS
    );
}

#[test]
fn submit_forwards_real_signer_privilege() {
    // A non-PDA account that signed the outer transaction keeps its signer privilege in
    // the executor CPI (the `account.is_signer()` pass-through). The executor transfers
    // from a real co-signer, which only succeeds if that signer status is forwarded.
    let authority = Keypair::new();
    let cosigner = Keypair::new();
    let recipient = Address::new_unique();

    let executor_instruction = transfer(&cosigner.pubkey(), &recipient, 0);
    let payload = SubmitPayload {
        signer_program_id: spl_ed25519_signer_interface::id(),
        executor_program_id: executor_instruction.program_id,
        executor_instruction_data: executor_instruction.data.clone(),
    };
    let envelope = SubmitEnvelope {
        signatures: vec![authority.sign_message(&payload.signing_bytes().unwrap())],
        payload,
    };
    let instruction = submit(
        envelope,
        &[authority.pubkey()],
        &executor_instruction.accounts,
    );

    init_mollusk().process_and_validate_instruction(
        &instruction,
        &[
            (authority.pubkey(), Account::default()),
            mollusk_svm::program::keyed_account_for_system_program(),
            (cosigner.pubkey(), Account::default()),
            (recipient, Account::default()),
        ],
        &[Check::success()],
    );
}

#[test]
fn sign_for_executor_builds_and_executes() {
    // Exercises the client signing path end-to-end: payload_for_executor + sign +
    // submit_for_executor, with no manual envelope plumbing.
    let authority = Keypair::new();
    let programmatic_signer = ProgrammaticSigner::derive_address(
        &spl_ed25519_signer_interface::id(),
        &authority.pubkey(),
    );
    let recipient = Address::new_unique();

    let executor_instruction = transfer(&programmatic_signer, &recipient, 0);

    let instruction =
        spl_ed25519_signer_client::signing::sign_and_submit(&executor_instruction, &[&authority])
            .unwrap();

    init_mollusk().process_and_validate_instruction(
        &instruction,
        &[
            (authority.pubkey(), Account::default()),
            mollusk_svm::program::keyed_account_for_system_program(),
            (programmatic_signer, Account::default()),
            (recipient, Account::default()),
        ],
        &[Check::success()],
    );
}
