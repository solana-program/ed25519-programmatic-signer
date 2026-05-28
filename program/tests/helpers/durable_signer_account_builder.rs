use {
    solana_account::Account, solana_address::Address, solana_rent::Rent,
    spl_ed25519_durable_signer_interface::state::DurableSignerAccount,
};

pub struct DurableSignerAccountBuilder {
    key: Address,
    owner: Address,
    lamports: Option<u64>,
    data: Option<Vec<u8>>,
}

impl Default for DurableSignerAccountBuilder {
    fn default() -> Self {
        Self {
            key: Address::from([1; 32]),
            owner: spl_ed25519_durable_signer_interface::id(),
            lamports: None,
            data: None,
        }
    }
}

impl DurableSignerAccountBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn key(mut self, key: Address) -> Self {
        self.key = key;
        self
    }

    pub fn owner(mut self, owner: Address) -> Self {
        self.owner = owner;
        self
    }

    pub fn lamports(mut self, lamports: u64) -> Self {
        self.lamports = Some(lamports);
        self
    }

    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = Some(data);
        self
    }

    pub fn build(self) -> (Address, Account) {
        let data = self
            .data
            // Default to program-owned, zero-filled account (waiting to be initialized)
            .unwrap_or_else(|| vec![0; DurableSignerAccount::LEN]);
        let lamports = self
            .lamports
            .unwrap_or_else(|| Rent::default().minimum_balance(data.len()));
        (
            self.key,
            Account {
                lamports,
                data,
                owner: self.owner,
                ..Account::default()
            },
        )
    }
}
