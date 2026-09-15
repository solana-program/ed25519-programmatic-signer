use {
    crate::helpers::common::{decode_state, init_mollusk, initialize_nonce_account_at},
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
    nonce_address: Address,
    nonce_account: Option<(Address, Account)>,
    excess_lamports: u64,
    current_nonce: Option<Hash>,
    transition_commitment: Hash,
    authority_is_signer: bool,
    advance_authority: Option<Address>,
    checks: Vec<Check<'a>>,
}

impl Default for AdvanceBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            authority: Address::from([2; 32]),
            nonce_address: Address::new_unique(),
            nonce_account: None,
            excess_lamports: 0,
            current_nonce: None,
            transition_commitment: Hash::new_from_array([3; 32]),
            authority_is_signer: true,
            advance_authority: None,
            checks: vec![],
        }
    }
}

impl<'a> AdvanceBuilder<'a> {
    /// Sets the address used when no nonce account override is supplied.
    pub fn nonce_address(mut self, nonce_address: Address) -> Self {
        self.nonce_address = nonce_address;
        self
    }

    pub fn initialization_slot(mut self, slot: u64) -> Self {
        self.mollusk.sysvars.clock.slot = slot;
        self
    }

    pub fn excess_lamports(mut self, lamports: u64) -> Self {
        self.excess_lamports = lamports;
        self
    }

    pub fn nonce_account(mut self, nonce_account: (Address, Account)) -> Self {
        self.nonce_account = Some(nonce_account);
        self
    }

    pub fn current_nonce(mut self, current_nonce: Hash) -> Self {
        self.current_nonce = Some(current_nonce);
        self
    }

    pub fn transition_commitment(mut self, transition_commitment: Hash) -> Self {
        self.transition_commitment = transition_commitment;
        self
    }

    pub fn authority_not_signer(mut self) -> Self {
        self.authority_is_signer = false;
        self
    }

    pub fn advance_authority(mut self, authority: Address) -> Self {
        self.advance_authority = Some(authority);
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> InstructionResult {
        let (nonce_account_address, nonce_account) =
            self.nonce_account.take().unwrap_or_else(|| {
                let (address, mut account) =
                    initialize_nonce_account_at(&self.mollusk, &self.authority, self.nonce_address);
                account.lamports = account.lamports.checked_add(self.excess_lamports).unwrap();
                (address, account)
            });
        let current_nonce = self
            .current_nonce
            .unwrap_or_else(|| decode_state(&nonce_account).nonce);
        let advance_authority = self.advance_authority.unwrap_or(self.authority);

        let mut instruction = advance(
            &advance_authority,
            &nonce_account_address,
            current_nonce,
            self.transition_commitment,
        );
        if !self.authority_is_signer {
            instruction.accounts[0] = AccountMeta::new_readonly(advance_authority, false);
        }

        let accounts = vec![
            (advance_authority, Account::default()),
            (nonce_account_address, nonce_account),
        ];

        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }

        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &self.checks)
    }
}
