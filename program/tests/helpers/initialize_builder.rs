use {
    crate::helpers::{
        common::init_mollusk, durable_signer_account_builder::DurableSignerAccountBuilder,
    },
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    spl_ed25519_durable_signer_client::instruction::initialize,
};

pub struct InitializeBuilder<'a> {
    mollusk: Mollusk,
    durable_signer: Option<(Address, Account)>,
    authority_addr: Option<Address>,
    instruction_data: Option<Vec<u8>>,
    checks: Vec<Check<'a>>,
}

impl Default for InitializeBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            durable_signer: None,
            authority_addr: None,
            instruction_data: None,
            checks: vec![],
        }
    }
}

impl<'a> InitializeBuilder<'a> {
    pub fn durable_signer(mut self, durable_signer: (Address, Account)) -> Self {
        self.durable_signer = Some(durable_signer);
        self
    }

    pub fn authority_addr(mut self, key: Address) -> Self {
        self.authority_addr = Some(key);
        self
    }

    pub fn instruction_data(mut self, data: Vec<u8>) -> Self {
        self.instruction_data = Some(data);
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> InstructionResult {
        let durable_signer = self
            .durable_signer
            .unwrap_or_else(|| DurableSignerAccountBuilder::default().build());
        let authority_addr = self
            .authority_addr
            .unwrap_or_else(|| Address::from([2; 32]));
        let slot_hashes = self.mollusk.sysvars.keyed_account_for_slot_hashes_sysvar();

        let mut instruction = initialize(&durable_signer.0, &authority_addr);
        if let Some(instruction_data) = self.instruction_data {
            instruction.data = instruction_data;
        }

        let accounts = vec![
            durable_signer,
            (authority_addr, Account::default()),
            slot_hashes,
        ];

        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }

        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &self.checks)
    }
}
