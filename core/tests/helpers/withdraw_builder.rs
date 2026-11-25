use {
    crate::helpers::common::{init_mollusk, initialize_nonce_account_at},
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    spl_nonce_client::instruction::withdraw,
};

enum Destination {
    Account((Address, Account)),
    Nonce,
    Authority,
}

pub struct WithdrawBuilder<'a> {
    mollusk: Mollusk,
    authority: Address,
    nonce_address: Address,
    nonce_account: Option<(Address, Account)>,
    destination: Destination,
    account_count: usize,
    excess_lamports: u64,
    amount: Option<u64>,
    withdrawal_slot: Option<u64>,
    authority_is_signer: bool,
    withdraw_authority: Option<Address>,
    checks: Vec<Check<'a>>,
}

impl Default for WithdrawBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            authority: Address::from([2; 32]),
            nonce_address: Address::new_unique(),
            nonce_account: None,
            destination: Destination::Account((
                Address::from([3; 32]),
                Account {
                    lamports: 1_000_000,
                    ..Account::default()
                },
            )),
            account_count: 3,
            excess_lamports: 100,
            amount: Some(100),
            withdrawal_slot: None,
            authority_is_signer: true,
            withdraw_authority: None,
            checks: vec![],
        }
    }
}

impl<'a> WithdrawBuilder<'a> {
    /// Sets the address used when no nonce account override is supplied.
    pub fn nonce_address(mut self, nonce_address: Address) -> Self {
        self.nonce_address = nonce_address;
        self
    }

    pub fn nonce_account(mut self, nonce_account: (Address, Account)) -> Self {
        self.nonce_account = Some(nonce_account);
        self
    }

    pub fn destination_account(mut self, destination_account: (Address, Account)) -> Self {
        self.destination = Destination::Account(destination_account);
        self
    }

    /// Reuses the nonce account as the withdrawal destination.
    pub fn destination_is_nonce(mut self) -> Self {
        self.destination = Destination::Nonce;
        self
    }

    /// Reuses the withdrawal authority account as the destination.
    pub fn destination_is_authority(mut self) -> Self {
        self.destination = Destination::Authority;
        self
    }

    /// Truncates the instruction account metas to exercise missing-account errors.
    pub fn account_count(mut self, count: usize) -> Self {
        self.account_count = count;
        self
    }

    pub fn initialization_slot(mut self, slot: u64) -> Self {
        self.mollusk.sysvars.clock.slot = slot;
        self
    }

    pub fn withdrawal_slot(mut self, slot: u64) -> Self {
        self.withdrawal_slot = Some(slot);
        self
    }

    pub fn excess_lamports(mut self, lamports: u64) -> Self {
        self.excess_lamports = lamports;
        self
    }

    pub fn amount(mut self, amount: u64) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn withdraw_all(mut self) -> Self {
        self.amount = None;
        self
    }

    pub fn authority_not_signer(mut self) -> Self {
        self.authority_is_signer = false;
        self
    }

    pub fn withdraw_authority(mut self, authority: Address) -> Self {
        self.withdraw_authority = Some(authority);
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> InstructionResult {
        let (nonce_address, nonce_account) = self.nonce_account.take().unwrap_or_else(|| {
            let (address, mut account) =
                initialize_nonce_account_at(&self.mollusk, &self.authority, self.nonce_address);
            account.lamports = account.lamports.checked_add(self.excess_lamports).unwrap();
            (address, account)
        });
        let authority = self.withdraw_authority.unwrap_or(self.authority);
        let amount = self.amount.unwrap_or(nonce_account.lamports);
        if let Some(slot) = self.withdrawal_slot {
            self.mollusk.sysvars.clock.slot = slot;
        }
        let destination_address = match &self.destination {
            Destination::Account((address, _)) => *address,
            Destination::Nonce => nonce_address,
            Destination::Authority => authority,
        };
        let mut instruction = withdraw(&authority, &nonce_address, &destination_address, amount);
        instruction.accounts[0].is_signer = self.authority_is_signer;
        instruction.accounts.truncate(self.account_count);

        let mut accounts = Vec::new();
        if authority != nonce_address {
            accounts.push((authority, Account::default()));
        }
        accounts.push((nonce_address, nonce_account));
        if let Destination::Account(destination) = self.destination {
            if !accounts
                .iter()
                .any(|(address, _)| *address == destination.0)
            {
                accounts.push(destination);
            }
        }
        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }
        self.mollusk
            .process_and_validate_instruction(&instruction, &accounts, &self.checks)
    }
}
