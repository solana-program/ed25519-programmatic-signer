use {
    solana_account::Account, solana_address::Address, solana_rent::Rent,
    spl_nonce_interface::state::Nonce,
};

pub struct NonceAccountBuilder {
    key: Address,
    owner: Address,
    data: Option<Vec<u8>>,
}

impl Default for NonceAccountBuilder {
    fn default() -> Self {
        Self {
            key: Address::from([1; 32]),
            owner: spl_nonce_interface::id(),
            data: None,
        }
    }
}

impl NonceAccountBuilder {
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

    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = Some(data);
        self
    }

    pub fn build(self) -> (Address, Account) {
        let data = self
            .data
            // Default to program-owned, zero-filled account (waiting to be initialized)
            .unwrap_or_else(|| vec![0; Nonce::LEN]);
        let lamports = Rent::default().minimum_balance(data.len());
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
