mod common;

use {
    common::{
        PAYER_LAMPORTS, TestContext, account, create_nonce_account, decode_state, init_context,
        process_outer_transaction, store_account, system_account, test_hash,
    },
    mollusk_svm::result::{Check, InstructionResult, ProgramResult},
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_program_error::ProgramError,
    solana_rent::Rent,
    solana_signer::Signer as _,
    solana_system_interface::instruction as system_instruction,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    spl_message_executor_interface::error::Error as MessageExecutorError,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::{
        Error, TransactionPlan, build_transaction,
        nonce::{advance_instruction, create_nonce_account_instructions},
        sign_transaction,
        submit::submit_transaction,
        verify::verify,
    },
};

const PDA_LAMPORTS: u64 = 100_000_000;
const TRANSFER_LAMPORTS: u64 = 1_000_000;

fn genesis_hash() -> Hash {
    test_hash(250)
}

struct Harness {
    context: TestContext,
    payer: Address,
    fee_payer: Keypair,
}

impl Harness {
    fn new() -> Self {
        let context = init_context();
        let payer = Address::new_unique();
        let fee_payer = Keypair::new();
        store_account(&context, payer, system_account(PAYER_LAMPORTS));
        store_account(&context, fee_payer.pubkey(), system_account(PAYER_LAMPORTS));
        Self {
            context,
            payer,
            fee_payer,
        }
    }

    fn create_nonce_account(&self, authority: &Address) -> Address {
        let nonce_account = Address::new_unique();
        create_nonce_account(&self.context, &self.payer, &nonce_account, authority);
        nonce_account
    }

    fn nonce_account_state(&self, nonce_account: &Address) -> Nonce {
        decode_state(&account(&self.context, nonce_account))
    }

    fn signed_transfer(
        &self,
        nonce_account: Address,
        authority: &Keypair,
        recipient: Address,
    ) -> VersionedTransaction {
        let transaction_plan = TransactionPlan::transfer(
            nonce_account,
            authority.pubkey(),
            recipient,
            TRANSFER_LAMPORTS,
        )
        .unwrap();
        let mut transaction = build_transaction(
            &transaction_plan,
            self.nonce_account_state(&nonce_account).nonce,
            genesis_hash(),
        )
        .unwrap();
        sign_transaction(&mut transaction, authority).unwrap();
        transaction
    }

    fn submit(
        &self,
        transaction: &VersionedTransaction,
        recent_blockhash: Hash,
    ) -> VersionedTransaction {
        self.submit_with_extra(transaction, &[], recent_blockhash)
    }

    fn submit_with_extra(
        &self,
        transaction: &VersionedTransaction,
        extra_signers: &[&dyn solana_signer::Signer],
        recent_blockhash: Hash,
    ) -> VersionedTransaction {
        submit_transaction(
            transaction,
            &self.fee_payer,
            extra_signers,
            recent_blockhash,
        )
        .unwrap()
    }

    fn process_success(&self, transaction: &VersionedTransaction) -> InstructionResult {
        process_outer_transaction(&self.context, transaction, &[Check::success()])
    }
}

fn programmatic_signer(authority: &Keypair) -> Address {
    ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority.pubkey())
}

fn fund_transfer_accounts(harness: &Harness, pda: Address, recipient: Address) {
    store_account(&harness.context, pda, system_account(PDA_LAMPORTS));
    store_account(&harness.context, recipient, system_account(0));
}

#[test]
fn nonce_account_setup() {
    let context = init_context();
    let payer = Address::new_unique();
    let nonce_account = Address::new_unique();
    let authority = Address::new_unique();
    store_account(&context, payer, system_account(PAYER_LAMPORTS));
    store_account(&context, nonce_account, system_account(0));

    let [create, initialize] = create_nonce_account_instructions(
        &payer,
        &nonce_account,
        &authority,
        Rent::default().minimum_balance(Nonce::LEN),
    );

    for instruction in [&create, &initialize] {
        assert!(
            instruction
                .accounts
                .iter()
                .all(|meta| meta.pubkey != authority || !meta.is_signer)
        );
    }
    assert!(
        initialize
            .accounts
            .iter()
            .any(|meta| meta.pubkey == authority)
    );

    context.process_and_validate_instruction_chain(&[
        (&create, &[Check::success()]),
        (&initialize, &[Check::success()]),
    ]);

    let state = decode_state(&account(&context, &nonce_account));
    assert_eq!(state.authority, authority);
    assert_ne!(state.nonce, Hash::default());
}

#[test]
fn transaction_end_to_end() {
    let harness = Harness::new();
    let authority = Keypair::new();
    let pda = programmatic_signer(&authority);
    let recipient = Address::new_unique();
    let nonce_account = harness.create_nonce_account(&pda);
    fund_transfer_accounts(&harness, pda, recipient);
    let old_nonce = harness.nonce_account_state(&nonce_account).nonce;

    let transaction = harness.signed_transfer(nonce_account, &authority, recipient);
    verify(
        &transaction,
        &harness.nonce_account_state(&nonce_account),
        &nonce_account,
        &genesis_hash(),
    )
    .unwrap();
    let transaction = harness.submit(&transaction, test_hash(2));

    harness.process_success(&transaction);

    assert_eq!(
        account(&harness.context, &recipient).lamports,
        TRANSFER_LAMPORTS
    );
    assert_eq!(
        account(&harness.context, &pda).lamports,
        PDA_LAMPORTS.checked_sub(TRANSFER_LAMPORTS).unwrap()
    );
    assert_ne!(harness.nonce_account_state(&nonce_account).nonce, old_nonce);
}

#[test]
fn replay_rejected() {
    let harness = Harness::new();
    let authority = Keypair::new();
    let pda = programmatic_signer(&authority);
    let recipient = Address::new_unique();
    let nonce_account = harness.create_nonce_account(&pda);
    fund_transfer_accounts(&harness, pda, recipient);
    let transaction = harness.signed_transfer(nonce_account, &authority, recipient);
    let transaction = harness.submit(&transaction, test_hash(3));

    harness.process_success(&transaction);
    expect_program_failure(
        &harness.context,
        &transaction,
        spl_message_executor_interface::id(),
        MessageExecutorError::NonceMismatch.into(),
    );
}

#[test]
fn cancel_keypair_authority() {
    let authority = Keypair::new();
    let nonce_account_authority = Keypair::new();
    let pda = programmatic_signer(&authority);

    let control = Harness::new();
    let control_recipient = Address::new_unique();
    let control_nonce_account = control.create_nonce_account(&nonce_account_authority.pubkey());
    fund_transfer_accounts(&control, pda, control_recipient);
    store_account(
        &control.context,
        nonce_account_authority.pubkey(),
        system_account(0),
    );
    let control_transaction = keypair_nonce_account_transaction(
        &control,
        control_nonce_account,
        &authority,
        &nonce_account_authority,
        control_recipient,
    );
    let control_transaction = control.submit_with_extra(
        &control_transaction,
        &[&nonce_account_authority],
        test_hash(4),
    );

    control.process_success(&control_transaction);
    assert_eq!(
        account(&control.context, &control_recipient).lamports,
        TRANSFER_LAMPORTS
    );

    let revoked = Harness::new();
    let revoked_recipient = Address::new_unique();
    let revoked_nonce_account = revoked.create_nonce_account(&nonce_account_authority.pubkey());
    fund_transfer_accounts(&revoked, pda, revoked_recipient);
    store_account(
        &revoked.context,
        nonce_account_authority.pubkey(),
        system_account(0),
    );
    let revoked_transaction = keypair_nonce_account_transaction(
        &revoked,
        revoked_nonce_account,
        &authority,
        &nonce_account_authority,
        revoked_recipient,
    );
    let revoked_transaction = revoked.submit_with_extra(
        &revoked_transaction,
        &[&nonce_account_authority],
        test_hash(5),
    );
    let advance = advance_instruction(
        &revoked_nonce_account,
        &nonce_account_authority.pubkey(),
        revoked.nonce_account_state(&revoked_nonce_account).nonce,
    );
    revoked
        .context
        .process_and_validate_instruction(&advance, &[Check::success()]);

    expect_program_failure(
        &revoked.context,
        &revoked_transaction,
        spl_message_executor_interface::id(),
        MessageExecutorError::NonceMismatch.into(),
    );
}

#[test]
fn cancel_pda_authority() {
    let harness = Harness::new();
    let authority = Keypair::new();
    let pda = programmatic_signer(&authority);
    let recipient = Address::new_unique();
    let nonce_account = harness.create_nonce_account(&pda);
    fund_transfer_accounts(&harness, pda, recipient);

    let prior = harness.signed_transfer(nonce_account, &authority, recipient);
    let prior_transaction = harness.submit(&prior, test_hash(6));
    let prior_nonce = harness.nonce_account_state(&nonce_account).nonce;
    let cancellation_plan =
        TransactionPlan::cancellation(nonce_account, authority.pubkey()).unwrap();
    let mut cancellation =
        build_transaction(&cancellation_plan, prior_nonce, genesis_hash()).unwrap();
    sign_transaction(&mut cancellation, &authority).unwrap();
    verify(
        &cancellation,
        &harness.nonce_account_state(&nonce_account),
        &nonce_account,
        &genesis_hash(),
    )
    .unwrap();
    let cancellation_transaction = harness.submit(&cancellation, test_hash(8));

    harness.process_success(&cancellation_transaction);
    let recent_slot_hash = harness
        .context
        .mollusk
        .sysvars
        .slot_hashes
        .first()
        .unwrap()
        .1;
    let expected_next_nonce = Nonce {
        nonce: prior_nonce,
        authority: pda,
    }
    .derive_next_nonce(
        &spl_nonce_interface::id(),
        &nonce_account,
        &recent_slot_hash,
    );
    assert_eq!(
        harness.nonce_account_state(&nonce_account).nonce,
        expected_next_nonce
    );
    expect_program_failure(
        &harness.context,
        &prior_transaction,
        spl_message_executor_interface::id(),
        MessageExecutorError::NonceMismatch.into(),
    );
}

#[test]
fn parallel_nonce_accounts() {
    run_parallel_nonce_accounts(true);
    run_parallel_nonce_accounts(false);
}

fn run_parallel_nonce_accounts(first_then_second: bool) {
    let harness = Harness::new();
    let authority = Keypair::new();
    let pda = programmatic_signer(&authority);
    let first_recipient = Address::new_unique();
    let second_recipient = Address::new_unique();
    let first_nonce_account = harness.create_nonce_account(&pda);
    let second_nonce_account = harness.create_nonce_account(&pda);
    fund_transfer_accounts(&harness, pda, first_recipient);
    store_account(&harness.context, second_recipient, system_account(0));
    let first_nonce = harness.nonce_account_state(&first_nonce_account).nonce;
    let second_nonce = harness.nonce_account_state(&second_nonce_account).nonce;

    let first = harness.signed_transfer(first_nonce_account, &authority, first_recipient);
    let second = harness.signed_transfer(second_nonce_account, &authority, second_recipient);
    let first_transaction = harness.submit(&first, test_hash(9));
    let second_transaction = harness.submit(&second, test_hash(10));

    if first_then_second {
        harness.process_success(&first_transaction);
        harness.process_success(&second_transaction);
    } else {
        harness.process_success(&second_transaction);
        harness.process_success(&first_transaction);
    }

    assert_eq!(
        account(&harness.context, &first_recipient).lamports,
        TRANSFER_LAMPORTS
    );
    assert_eq!(
        account(&harness.context, &second_recipient).lamports,
        TRANSFER_LAMPORTS
    );
    assert_eq!(
        account(&harness.context, &pda).lamports,
        PDA_LAMPORTS
            .checked_sub(TRANSFER_LAMPORTS.checked_mul(2).unwrap())
            .unwrap()
    );
    assert_ne!(
        harness.nonce_account_state(&first_nonce_account).nonce,
        first_nonce
    );
    assert_ne!(
        harness.nonce_account_state(&second_nonce_account).nonce,
        second_nonce
    );
}

#[test]
fn designated_relayer() {
    let harness = Harness::new();
    let authority = Keypair::new();
    let submit_signer = Keypair::new();
    let pda = programmatic_signer(&authority);
    let recipient = Address::new_unique();
    let nonce_account = harness.create_nonce_account(&pda);
    fund_transfer_accounts(&harness, pda, recipient);
    store_account(&harness.context, submit_signer.pubkey(), system_account(0));
    let instruction = system_instruction::transfer(&pda, &recipient, TRANSFER_LAMPORTS);
    let transaction_plan = TransactionPlan::new(
        vec![instruction],
        vec![authority.pubkey()],
        vec![submit_signer.pubkey()],
        nonce_account,
    )
    .unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        harness.nonce_account_state(&nonce_account).nonce,
        genesis_hash(),
    )
    .unwrap();
    sign_transaction(&mut transaction, &authority).unwrap();
    sign_transaction(&mut transaction, &submit_signer).unwrap();
    verify(
        &transaction,
        &harness.nonce_account_state(&nonce_account),
        &nonce_account,
        &genesis_hash(),
    )
    .unwrap();

    assert_eq!(
        submit_transaction(&transaction, &harness.fee_payer, &[], test_hash(12)),
        Err(Error::MissingOuterSigner(submit_signer.pubkey()))
    );

    let positive = harness.submit_with_extra(&transaction, &[&submit_signer], test_hash(13));
    harness.process_success(&positive);

    assert_eq!(
        account(&harness.context, &recipient).lamports,
        TRANSFER_LAMPORTS
    );
}

fn keypair_nonce_account_transaction(
    harness: &Harness,
    nonce_account: Address,
    authority: &Keypair,
    nonce_account_authority: &Keypair,
    recipient: Address,
) -> VersionedTransaction {
    let instruction = system_instruction::transfer(
        &programmatic_signer(authority),
        &recipient,
        TRANSFER_LAMPORTS,
    );
    let transaction_plan = TransactionPlan::new(
        vec![instruction],
        vec![authority.pubkey()],
        vec![nonce_account_authority.pubkey()],
        nonce_account,
    )
    .unwrap();
    let mut transaction = build_transaction(
        &transaction_plan,
        harness.nonce_account_state(&nonce_account).nonce,
        genesis_hash(),
    )
    .unwrap();
    sign_transaction(&mut transaction, authority).unwrap();
    sign_transaction(&mut transaction, nonce_account_authority).unwrap();
    verify(
        &transaction,
        &harness.nonce_account_state(&nonce_account),
        &nonce_account,
        &genesis_hash(),
    )
    .unwrap();
    transaction
}

fn expect_program_failure(
    context: &TestContext,
    transaction: &VersionedTransaction,
    expected_program: Address,
    expected_error: ProgramError,
) {
    if let Some(logger) = &context.mollusk.logger {
        logger.borrow_mut().messages.clear();
    }
    let result = process_outer_transaction(context, transaction, &[]);
    assert_eq!(
        result.program_result,
        ProgramResult::Failure(expected_error.clone())
    );
    let Some(logger) = &context.mollusk.logger else {
        panic!("mollusk logger is not configured");
    };
    let logs = logger.borrow().messages.join("\n");
    let ProgramError::Custom(code) = expected_error else {
        panic!("expected custom program error");
    };
    let failed_line = logs
        .lines()
        .find(|line| line.contains(&format!("Program {expected_program} failed")))
        .unwrap_or_else(|| panic!("missing failing program log for {expected_program}\n{logs}"));
    assert!(
        failed_line.contains(&format!("custom program error: 0x{code:x}")),
        "missing custom error {code} in failing program log\n{logs}"
    );
}
