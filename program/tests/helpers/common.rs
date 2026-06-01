use {
    crate::helpers::durable_signer_account_builder::DurableSignerAccountBuilder,
    mollusk_svm::Mollusk,
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    spl_ed25519_durable_signer_client::instruction::initialize,
    spl_ed25519_durable_signer_interface::state::DurableSignerAccount,
};

pub fn init_mollusk() -> Mollusk {
    Mollusk::new(
        &spl_ed25519_durable_signer_interface::id(),
        "spl_ed25519_durable_signer_program",
    )
}

pub fn decode_state(account: &Account) -> DurableSignerAccount {
    wincode::deserialize_exact(&account.data).unwrap()
}

pub fn initialize_durable_signer(mollusk: &Mollusk, authority: &Address) -> (Address, Account) {
    let durable_signer = DurableSignerAccountBuilder::new()
        .key(Address::new_unique())
        .build();
    let instruction = initialize(&durable_signer.0, authority);
    let result = mollusk.process_and_validate_instruction(
        &instruction,
        &[
            durable_signer,
            (*authority, signer_account(0)),
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

pub fn signer_account(lamports: u64) -> Account {
    Account {
        lamports,
        data: vec![],
        owner: solana_system_interface::program::id(),
        executable: false,
        rent_epoch: u64::MAX,
    }
}

pub fn writable_system_account(lamports: u64) -> Account {
    signer_account(lamports)
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

pub fn advance_slot_hash(mollusk: &mut Mollusk, tag: u8) -> Hash {
    let slot = mollusk.sysvars.clock.slot.saturating_add(1);
    let hash = Hash::new_from_array([tag; 32]);
    mollusk.sysvars.clock.slot = slot;
    mollusk.sysvars.slot_hashes.add(slot, hash);
    hash
}
