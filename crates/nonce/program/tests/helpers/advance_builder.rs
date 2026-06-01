use {
    crate::helpers::common::{decode_state, init_mollusk, initialize_nonce_account},
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::AccountMeta,
    spl_nonce_client::instruction::advance,
};

pub struct AdvanceBuilder<'a> {
    mollusk: Mollusk,
    authority: Address,
    nonce_account: Option<(Address, Account)>,
    current_nonce: Option<Hash>,
    authority_is_signer: bool,
    advance_authority: Option<Address>,
    checks: Vec<Check<'a>>,
}

impl Default for AdvanceBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            authority: Address::from([2; 32]),
            nonce_account: None,
            current_nonce: None,
            authority_is_signer: true,
            advance_authority: None,
            checks: vec![],
        }
    }
}

impl<'a> AdvanceBuilder<'a> {
    pub fn nonce_account(mut self, nonce_account: (Address, Account)) -> Self {
        self.nonce_account = Some(nonce_account);
        self
    }

    pub fn current_nonce(mut self, current_nonce: Hash) -> Self {
        self.current_nonce = Some(current_nonce);
        self
    }

    pub fn authority_not_signer(mut self) -> Self {
        self.authority_is_signer = false;
        self
    }

    /// Uses a different authority account for the `Advance` call than the one stored.
    pub fn advance_authority(mut self, authority: Address) -> Self {
        self.advance_authority = Some(authority);
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> InstructionResult {
        let nonce_account = self
            .nonce_account
            .take()
            .unwrap_or_else(|| initialize_nonce_account(&self.mollusk, &self.authority));
        let current_nonce = self
            .current_nonce
            .unwrap_or_else(|| decode_state(&nonce_account.1).nonce);
        let advance_authority = self.advance_authority.unwrap_or(self.authority);

        let mut instruction = advance(&nonce_account.0, &advance_authority, current_nonce);
        if !self.authority_is_signer {
            instruction.accounts[1] = AccountMeta::new_readonly(advance_authority, false);
        }

        let accounts = vec![
            nonce_account,
            (advance_authority, Account::default()),
            self.mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
        ];

        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }

        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &self.checks)
    }
}
