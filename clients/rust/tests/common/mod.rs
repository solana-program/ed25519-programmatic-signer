use {
    mollusk_svm::{
        Mollusk, MolluskContext,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_rent::Rent,
    solana_transaction::versioned::VersionedTransaction,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::nonce::create_nonce_account_instructions,
    std::collections::HashMap,
};

pub const PAYER_LAMPORTS: u64 = 10_000_000_000;

pub type TestContext = MolluskContext<HashMap<Address, Account>>;

pub fn init_mollusk() -> Mollusk {
    let mut mollusk = Mollusk::new(
        &spl_ed25519_signer_interface::id(),
        "spl_ed25519_signer_program",
    );
    mollusk.logger = Some(std::rc::Rc::default());
    mollusk.add_program(
        &spl_message_executor_interface::id(),
        "spl_message_executor_program",
    );
    mollusk.add_program(&spl_nonce_interface::id(), "spl_nonce_program");
    mollusk
}

pub fn init_context() -> TestContext {
    init_mollusk().with_context(HashMap::<Address, Account>::new())
}

pub fn create_nonce_account(
    context: &TestContext,
    payer: &Address,
    nonce_account: &Address,
    authority: &Address,
) -> (Address, Account) {
    store_account(context, *nonce_account, system_account(0));
    let [create, initialize] = create_nonce_account_instructions(
        payer,
        nonce_account,
        authority,
        Rent::default().minimum_balance(Nonce::LEN),
    );

    let result = context.process_and_validate_instruction_chain(&[
        (&create, &[Check::success()]),
        (&initialize, &[Check::success()]),
    ]);

    (
        *nonce_account,
        result.get_account(nonce_account).unwrap().clone(),
    )
}

pub fn decode_state(account: &Account) -> Nonce {
    wincode::deserialize_exact(&account.data).unwrap()
}

pub fn process_outer_transaction(
    context: &TestContext,
    transaction: &VersionedTransaction,
    checks: &[Check],
) -> InstructionResult {
    transaction.verify_and_hash_message().unwrap();

    let [compiled_instruction] = transaction.message.instructions() else {
        panic!("expected a single outer instruction");
    };
    let account_keys = transaction.message.static_account_keys();
    let program_id = *account_keys
        .get(usize::from(compiled_instruction.program_id_index))
        .unwrap();
    let accounts = compiled_instruction
        .accounts
        .iter()
        .map(|account_index| {
            let index = usize::from(*account_index);
            AccountMeta {
                pubkey: *account_keys.get(index).unwrap(),
                is_signer: transaction.message.is_signer(index),
                is_writable: is_header_writable(index, &transaction.message),
            }
        })
        .collect();
    let instruction = Instruction {
        program_id,
        accounts,
        data: compiled_instruction.data.clone(),
    };

    context.process_and_validate_instruction(&instruction, checks)
}

pub fn store_account(context: &TestContext, key: Address, account: Account) {
    context.account_store.borrow_mut().insert(key, account);
}

pub fn account(context: &TestContext, key: &Address) -> Account {
    context.account_store.borrow().get(key).unwrap().clone()
}

pub fn system_account(lamports: u64) -> Account {
    Account {
        lamports,
        data: Vec::new(),
        owner: solana_system_interface::program::id(),
        executable: false,
        rent_epoch: u64::MAX,
    }
}

pub fn test_hash(byte: u8) -> Hash {
    Hash::new_from_array([byte; 32])
}

fn is_header_writable(index: usize, message: &solana_message::VersionedMessage) -> bool {
    let header = message.header();
    let account_keys = message.static_account_keys();
    let required_signatures = usize::from(header.num_required_signatures);
    let writable_signers_end =
        required_signatures.saturating_sub(usize::from(header.num_readonly_signed_accounts));
    let writable_unsigned_end = account_keys
        .len()
        .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
    index < writable_signers_end || (required_signatures..writable_unsigned_end).contains(&index)
}
