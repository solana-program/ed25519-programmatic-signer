use {
    crate::helpers::{
        common::{MOCK_PROGRAM_ID, decode_state, init_mollusk, initialize_nonce_account},
        mock_auth::MockAuth,
    },
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::v1,
    solana_program_error::ProgramError,
    solana_system_interface::instruction::transfer,
    spl_programmatic_signer_core::{
        instruction::execute, message::execution_message, pda::ProgrammaticSigner,
    },
    std::collections::BTreeMap,
};

pub const DEFAULT_TRANSFER_LAMPORTS: u64 = 1_000_000;

type MessageMutation = Box<dyn FnOnce(&mut v1::Message)>;
type AuthorizationsMutation = Box<dyn FnOnce(&mut Vec<MockAuth>)>;
type IxMutation = Box<dyn FnOnce(&mut Instruction)>;

pub fn funded_account() -> Account {
    Account {
        lamports: 100_000_000,
        ..Account::default()
    }
}

/// Builds and runs an `Execute` instruction in Mollusk using `MockScheme`.
pub struct ExecuteBuilder<'a> {
    mollusk: Mollusk,
    authorities: Vec<Address>,
    nonce_account: Option<(Address, Account)>,
    nonce_authority: Option<Address>,
    expected_nonce: Option<Hash>,
    payload_ixs: Option<Vec<Instruction>>,
    recipient: Address,
    message_override: Option<v1::Message>,
    message_mutations: Vec<MessageMutation>,
    authorizations_mutations: Vec<AuthorizationsMutation>,
    transaction_signers: Vec<Address>,
    execute_ix_mutations: Vec<IxMutation>,
    account_overrides: Vec<(Address, Account)>,
    checks: Vec<Check<'a>>,
}

impl Default for ExecuteBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            authorities: vec![],
            nonce_account: None,
            nonce_authority: None,
            expected_nonce: None,
            payload_ixs: None,
            recipient: Address::new_unique(),
            message_override: None,
            message_mutations: vec![],
            authorizations_mutations: vec![],
            transaction_signers: vec![],
            execute_ix_mutations: vec![],
            account_overrides: vec![],
            checks: vec![],
        }
    }
}

impl<'a> ExecuteBuilder<'a> {
    pub fn authority(mut self, authority: Address) -> Self {
        self.authorities.push(authority);
        self
    }

    pub fn nonce_account(mut self, address: Address, account: Account) -> Self {
        self.nonce_account = Some((address, account));
        self
    }

    pub fn nonce_authority(mut self, authority: Address) -> Self {
        self.nonce_authority = Some(authority);
        self
    }

    pub fn expected_nonce(mut self, nonce: Hash) -> Self {
        self.expected_nonce = Some(nonce);
        self
    }

    pub fn payload_ix(mut self, ix: Instruction) -> Self {
        self.payload_ixs.get_or_insert_with(Vec::new).push(ix);
        self
    }

    pub fn payload_ixs(mut self, ixs: Vec<Instruction>) -> Self {
        self.payload_ixs = Some(ixs);
        self
    }

    pub fn recipient(mut self, recipient: Address) -> Self {
        self.recipient = recipient;
        self
    }

    pub fn message(mut self, message: v1::Message) -> Self {
        self.message_override = Some(message);
        self
    }

    pub fn mutate_message(mut self, mutation: impl FnOnce(&mut v1::Message) + 'static) -> Self {
        self.message_mutations.push(Box::new(mutation));
        self
    }

    pub fn mutate_authorizations(
        mut self,
        mutation: impl FnOnce(&mut Vec<MockAuth>) + 'static,
    ) -> Self {
        self.authorizations_mutations.push(Box::new(mutation));
        self
    }

    /// Marks `signer` as an outer transaction signer and authorizes it with
    /// `MockAuth::TransactionSigner`.
    pub fn transaction_signer(mut self, signer: Address) -> Self {
        self.transaction_signers.push(signer);
        self
    }

    pub fn mutate_execute_ix(mut self, mutation: impl FnOnce(&mut Instruction) + 'static) -> Self {
        self.execute_ix_mutations.push(Box::new(mutation));
        self
    }

    pub fn account(mut self, address: Address, account: Account) -> Self {
        self.account_overrides.push((address, account));
        self
    }

    pub fn funded_account(self, address: Address) -> Self {
        self.account(address, funded_account())
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn check_err(self, error: impl Into<ProgramError>) -> Self {
        self.check(Check::err(error.into()))
    }

    pub fn execute(self) -> ExecuteResult {
        let Self {
            mollusk,
            authorities,
            nonce_account,
            nonce_authority,
            expected_nonce,
            payload_ixs,
            recipient,
            message_override,
            message_mutations,
            authorizations_mutations,
            transaction_signers,
            execute_ix_mutations,
            account_overrides,
            mut checks,
        } = self;

        let authorities = if authorities.is_empty() {
            vec![Address::new_unique()]
        } else {
            authorities
        };
        let primary_pda = ProgrammaticSigner::derive_address(&MOCK_PROGRAM_ID, &authorities[0]);
        let nonce_authority = nonce_authority.unwrap_or(primary_pda);
        let (nonce_address, initial_nonce_account) =
            nonce_account.unwrap_or_else(|| initialize_nonce_account(&mollusk, &nonce_authority));
        let expected_nonce =
            expected_nonce.unwrap_or_else(|| decode_state(&initial_nonce_account).nonce);

        let mut message = message_override.unwrap_or_else(|| {
            let payload = payload_ixs.unwrap_or_else(|| {
                vec![transfer(
                    &primary_pda,
                    &recipient,
                    DEFAULT_TRANSFER_LAMPORTS,
                )]
            });
            execution_message(
                &MOCK_PROGRAM_ID,
                &nonce_address,
                &nonce_authority,
                expected_nonce,
                &payload,
                &authorities,
                &[],
            )
            .unwrap()
        });
        for mutation in message_mutations {
            mutation(&mut message);
        }

        let authorizations = build_authorizations(
            &message,
            &authorities,
            &transaction_signers,
            authorizations_mutations,
        );

        let mut ix = execute(&MOCK_PROGRAM_ID, authorizations, &message);
        for signer in &transaction_signers {
            let meta = ix
                .accounts
                .iter_mut()
                .find(|meta| meta.pubkey == *signer)
                .expect("transaction_signer must appear in execute instruction accounts");
            meta.is_signer = true;
        }
        for mutation in execute_ix_mutations {
            mutation(&mut ix);
        }

        let accounts = resolve_accounts(
            &ix,
            primary_pda,
            nonce_address,
            &initial_nonce_account,
            account_overrides,
        );

        if checks.is_empty() {
            checks.push(Check::success());
        }

        let raw = mollusk.process_and_validate_instruction(&ix, &accounts, &checks);
        let nonce_account = raw
            .get_account(&nonce_address)
            .unwrap_or(&initial_nonce_account)
            .clone();

        ExecuteResult {
            programmatic_signer: primary_pda,
            recipient,
            nonce_address,
            nonce_account,
            message,
            raw,
        }
    }
}

/// Builds one `MockAuth` per required signer in `message.account_keys` order: `Programmatic` for
/// derived PDAs, `TransactionSigner` for outer tx signers, and `None` otherwise, then applies any
/// test mutations.
fn build_authorizations(
    message: &v1::Message,
    authorities: &[Address],
    transaction_signers: &[Address],
    mutations: Vec<AuthorizationsMutation>,
) -> Vec<MockAuth> {
    let required = usize::from(message.header.num_required_signatures);
    let pdas: Vec<_> = authorities
        .iter()
        .map(|&authority| {
            (
                ProgrammaticSigner::derive_address(&MOCK_PROGRAM_ID, &authority),
                authority,
            )
        })
        .collect();

    let mut authorizations: Vec<MockAuth> = message
        .account_keys
        .iter()
        .take(required)
        .map(|key| {
            if let Some(&(_, authority)) = pdas.iter().find(|(pda, _)| pda == key) {
                MockAuth::Programmatic(authority)
            } else if transaction_signers.contains(key) {
                MockAuth::TransactionSigner
            } else {
                MockAuth::None
            }
        })
        .collect();

    for mutation in mutations {
        mutation(&mut authorizations);
    }
    authorizations
}

/// Builds the Mollusk account list matching `ix.accounts`, pre-populating the primary PDA,
/// system program, mock program, nonce account, and any test overrides.
fn resolve_accounts(
    ix: &Instruction,
    primary_pda: Address,
    nonce_address: Address,
    initial_nonce_account: &Account,
    account_overrides: Vec<(Address, Account)>,
) -> Vec<(Address, Account)> {
    let mut accounts_by_address = BTreeMap::from([
        (primary_pda, funded_account()),
        mollusk_svm::program::keyed_account_for_system_program(),
        (
            MOCK_PROGRAM_ID,
            mollusk_svm::program::create_program_account_loader_v3(&MOCK_PROGRAM_ID),
        ),
        (nonce_address, initial_nonce_account.clone()),
    ]);
    accounts_by_address.extend(account_overrides);

    ix.accounts
        .iter()
        .map(|meta| {
            (
                meta.pubkey,
                accounts_by_address
                    .get(&meta.pubkey)
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect()
}

pub struct ExecuteResult {
    pub programmatic_signer: Address,
    pub recipient: Address,
    pub nonce_address: Address,
    pub nonce_account: Account,
    pub message: v1::Message,
    raw: InstructionResult,
}

impl ExecuteResult {
    pub fn account(&self, key: &Address) -> Option<&Account> {
        self.raw.get_account(key)
    }
}
