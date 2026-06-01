use {
    crate::helpers::nonce_account_builder::NonceAccountBuilder,
    mollusk_svm::Mollusk,
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    spl_nonce_client::instruction::initialize,
    spl_nonce_interface::state::Nonce,
};

pub fn init_mollusk() -> Mollusk {
    let mut mollusk = Mollusk::new(
        &spl_message_executor_interface::id(),
        "spl_message_executor_program",
    );
    mollusk.add_program(&spl_nonce_interface::id(), "spl_nonce_program");
    mollusk
}

pub fn decode_state(account: &Account) -> Nonce {
    wincode::deserialize_exact(&account.data).unwrap()
}

pub fn initialize_nonce_account(mollusk: &Mollusk, authority: &Address) -> (Address, Account) {
    let nonce_account = NonceAccountBuilder::new()
        .key(Address::new_unique())
        .build();
    let instruction = initialize(&nonce_account.0, authority);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            nonce_account,
            (*authority, system_account(0)),
            mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ],
        &[mollusk_svm::result::Check::success()],
    );

    (
        instruction.accounts[0].pubkey,
        result
            .get_account(&instruction.accounts[0].pubkey)
            .unwrap()
            .clone(),
    )
}

pub fn keyed_account_for_nonce_program() -> (Address, Account) {
    (
        spl_nonce_interface::id(),
        mollusk_svm::program::create_program_account_loader_v3(&spl_nonce_interface::id()),
    )
}

pub fn system_account(lamports: u64) -> Account {
    Account {
        lamports,
        data: vec![],
        owner: solana_system_interface::program::id(),
        executable: false,
        rent_epoch: u64::MAX,
    }
}

pub fn system_transfer_instruction(from: Address, to: Address, lamports: u64) -> Instruction {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: solana_system_interface::program::id(),
        accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, false)],
        data,
    }
}

/// Converts the crates.io `Hash` into the message crate's `Hash` (the pinned SDK ships
/// its own `solana-hash`).
pub fn message_hash(hash: Hash) -> solana_message::Hash {
    solana_message::Hash::new_from_array(*hash.as_bytes())
}

pub fn compiled_transfer_instruction(
    from_index: u8,
    to_index: u8,
    program_id_index: u8,
    lamports: u64,
) -> solana_message::compiled_instruction::CompiledInstruction {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    solana_message::compiled_instruction::CompiledInstruction {
        program_id_index,
        accounts: vec![from_index, to_index],
        data,
    }
}
