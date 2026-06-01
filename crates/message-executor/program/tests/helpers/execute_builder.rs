use {
    crate::helpers::common::{
        decode_state, init_mollusk, initialize_nonce_account, keyed_account_for_nonce_program,
        message_hash, system_account,
    },
    mollusk_svm::{
        Mollusk,
        result::{Check, InstructionResult},
    },
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::{
        AccountMeta as MessageAccountMeta, Instruction as MessageInstruction, Message,
        VersionedMessage,
    },
    spl_message_executor_client::instruction::execute,
};

const DEFAULT_AUTHORITY_LAMPORTS: u64 = 100_000_000;

/// Drives `Execute` directly at transaction level. Required signers are plain addresses
/// whose signer privilege comes from the instruction's account metas. No signer program
/// or signatures involved.
pub struct ExecuteBuilder<'a> {
    mollusk: Mollusk,
    nonce_account: Option<(Address, Account)>,
    authority: Address,
    inner_instructions: Vec<MessageInstruction>,
    recent_blockhash: Option<Hash>,
    message: Option<VersionedMessage>,
    execute_instruction: Option<Instruction>,
    authority_lamports: u64,
    accounts: Vec<(Address, Account)>,
    checks: Vec<Check<'a>>,
}

impl Default for ExecuteBuilder<'_> {
    fn default() -> Self {
        Self {
            mollusk: init_mollusk(),
            nonce_account: None,
            authority: Address::from([3; 32]),
            inner_instructions: vec![],
            recent_blockhash: None,
            message: None,
            execute_instruction: None,
            authority_lamports: DEFAULT_AUTHORITY_LAMPORTS,
            accounts: vec![],
            checks: vec![],
        }
    }
}

impl<'a> ExecuteBuilder<'a> {
    pub fn nonce_account(mut self, nonce_account: (Address, Account)) -> Self {
        self.nonce_account = Some(nonce_account);
        self
    }

    pub fn authority(mut self, authority: Address) -> Self {
        self.authority = authority;
        self
    }

    pub fn inner_instruction(mut self, instruction: Instruction) -> Self {
        self.inner_instructions
            .push(message_instruction(instruction));
        self
    }

    pub fn recent_blockhash(mut self, recent_blockhash: Hash) -> Self {
        self.recent_blockhash = Some(recent_blockhash);
        self
    }

    pub fn message(mut self, message: VersionedMessage) -> Self {
        self.message = Some(message);
        self
    }

    pub fn execute_instruction(mut self, instruction: Instruction) -> Self {
        self.execute_instruction = Some(instruction);
        self
    }

    pub fn authority_lamports(mut self, lamports: u64) -> Self {
        self.authority_lamports = lamports;
        self
    }

    pub fn account(mut self, key: Address, account: Account) -> Self {
        self.accounts.push((key, account));
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> ExecuteResult {
        let nonce_account = self
            .nonce_account
            .take()
            .unwrap_or_else(|| initialize_nonce_account(&self.mollusk, &self.authority));
        let recent_blockhash = self
            .recent_blockhash
            .unwrap_or_else(|| decode_state(&nonce_account.1).nonce);

        let message = self.message.take().unwrap_or_else(|| {
            VersionedMessage::Legacy(Message::new_with_blockhash(
                &self.inner_instructions,
                Some(&self.authority),
                &message_hash(recent_blockhash),
            ))
        });
        let instruction = self
            .execute_instruction
            .take()
            .unwrap_or_else(|| execute(&nonce_account.0, &message));

        let accounts = self.assemble_accounts(&instruction, nonce_account.clone(), &message);
        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }
        let raw =
            self.mollusk
                .process_and_validate_instruction(&instruction, &accounts, &self.checks);
        let nonce_account = (
            nonce_account.0,
            raw.get_account(&nonce_account.0)
                .unwrap_or(&nonce_account.1)
                .clone(),
        );

        ExecuteResult {
            nonce_account,
            message,
            instruction,
            raw,
        }
    }

    fn assemble_accounts(
        &self,
        instruction: &Instruction,
        nonce_account: (Address, Account),
        message: &VersionedMessage,
    ) -> Vec<(Address, Account)> {
        let mut accounts = vec![nonce_account];
        let mut push_unique = |key: Address, account: Account| {
            if accounts.iter().any(|(candidate, _)| candidate == &key) {
                return;
            }
            accounts.push((key, account));
        };

        let nonce_program = keyed_account_for_nonce_program();
        push_unique(nonce_program.0, nonce_program.1);
        push_unique(
            solana_sdk_ids::sysvar::slot_hashes::ID,
            self.mollusk
                .sysvars
                .keyed_account_for_slot_hashes_sysvar()
                .1,
        );
        push_unique(
            solana_system_interface::program::id(),
            mollusk_svm::program::keyed_account_for_system_program().1,
        );
        push_unique(self.authority, system_account(self.authority_lamports));
        for (key, account) in &self.accounts {
            push_unique(*key, account.clone());
        }
        for key in message.static_account_keys() {
            push_unique(*key, system_account(0));
        }
        for meta in &instruction.accounts {
            push_unique(meta.pubkey, system_account(0));
        }

        accounts
    }
}

pub struct ExecuteResult {
    pub nonce_account: (Address, Account),
    pub message: VersionedMessage,
    pub instruction: Instruction,
    pub raw: InstructionResult,
}

impl ExecuteResult {
    pub fn account(&self, key: &Address) -> Option<&Account> {
        self.raw.get_account(key)
    }
}

/// Converts a client-built instruction into the message crate's instruction type.
pub fn message_instruction(instruction: Instruction) -> MessageInstruction {
    MessageInstruction {
        program_id: instruction.program_id,
        accounts: instruction
            .accounts
            .into_iter()
            .map(|meta| MessageAccountMeta {
                pubkey: meta.pubkey,
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: instruction.data,
    }
}
