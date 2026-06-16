use {
    crate::helpers::{common::init_mollusk, signer_context_builder::SignerContextBuilder},
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    spl_ed25519_programmatic_signer_client::instruction::initialize,
};

pub struct InitializeBuilder<'a> {
    mollusk: Mollusk,
    signer_context: Option<(Address, Account)>,
    authority_address: Option<Address>,
    instruction_data: Option<Vec<u8>>,
    checks: Vec<Check<'a>>,
}

impl Default for InitializeBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            signer_context: None,
            authority_address: None,
            instruction_data: None,
            checks: vec![],
        }
    }
}

impl<'a> InitializeBuilder<'a> {
    pub fn signer_context(mut self, signer_context: (Address, Account)) -> Self {
        self.signer_context = Some(signer_context);
        self
    }

    pub fn authority_addr(mut self, key: Address) -> Self {
        self.authority_address = Some(key);
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
        let signer_context = self
            .signer_context
            .unwrap_or_else(|| SignerContextBuilder::default().build());
        let authority_address = self
            .authority_address
            .unwrap_or_else(|| Address::from([2; 32]));
        let slot_hashes = self.mollusk.sysvars.keyed_account_for_slot_hashes_sysvar();

        let mut instruction = initialize(&signer_context.0, &authority_address);
        if let Some(instruction_data) = self.instruction_data {
            instruction.data = instruction_data;
        }

        let accounts = vec![
            signer_context,
            (authority_address, Account::default()),
            slot_hashes,
        ];

        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }

        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &self.checks)
    }
}
