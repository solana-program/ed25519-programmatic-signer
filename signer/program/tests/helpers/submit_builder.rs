use {
    crate::helpers::common::init_mollusk,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address,
    solana_keypair::Keypair,
    solana_program_error::ProgramError,
    solana_signature::Signature,
    solana_signer::{Signer, signers::Signers as _},
    solana_system_interface::instruction::transfer,
    spl_ed25519_signer_client::instruction::submit,
    spl_ed25519_signer_interface::{
        instruction::{SubmitEnvelope, SubmitPayload},
        pda::ProgrammaticSigner,
    },
    std::iter::once,
};

pub const DEFAULT_PDA_LAMPORTS: u64 = 100_000_000;
pub const DEFAULT_TRANSFER_LAMPORTS: u64 = 1_000_000;

pub struct SubmitBuilder<'a> {
    authority: Keypair,
    additional_authorities: Vec<Keypair>,
    recipient: Address,
    signer_program_id: Option<Address>,
    executor_program_id: Option<Address>,
    tampered_executor_data: Option<Vec<u8>>,
    sign_overrides: Vec<(usize, Keypair)>,
    signatures_override: Option<Vec<Signature>>,
    executor: Option<(Address, Account)>,
    authority_accounts_only: bool,
    checks: Vec<Check<'a>>,
}

impl Default for SubmitBuilder<'_> {
    fn default() -> Self {
        Self {
            authority: Keypair::new(),
            additional_authorities: vec![],
            recipient: Address::new_unique(),
            signer_program_id: None,
            executor_program_id: None,
            tampered_executor_data: None,
            sign_overrides: vec![],
            signatures_override: None,
            executor: None,
            authority_accounts_only: false,
            checks: vec![],
        }
    }
}

impl<'a> SubmitBuilder<'a> {
    pub fn additional_authority(mut self, authority: Keypair) -> Self {
        self.additional_authorities.push(authority);
        self
    }

    pub fn signer_program_id(mut self, id: Address) -> Self {
        self.signer_program_id = Some(id);
        self
    }

    pub fn executor_program_id(mut self, id: Address) -> Self {
        self.executor_program_id = Some(id);
        self
    }

    pub fn unsigned_executor_data(mut self, data: Vec<u8>) -> Self {
        self.tampered_executor_data = Some(data);
        self
    }

    pub fn signed_by(mut self, index: usize, key: Keypair) -> Self {
        self.sign_overrides.push((index, key));
        self
    }

    pub fn signatures(mut self, signatures: Vec<Signature>) -> Self {
        self.signatures_override = Some(signatures);
        self
    }

    pub fn executor(mut self, address: Address, account: Account) -> Self {
        self.executor = Some((address, account));
        self
    }

    pub fn authority_accounts_only(mut self) -> Self {
        self.authority_accounts_only = true;
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn check_err(self, error: impl Into<ProgramError>) -> Self {
        self.check(Check::err(error.into()))
    }

    pub fn execute(mut self) -> SubmitResult {
        let signers: Vec<&Keypair> = once(&self.authority)
            .chain(self.additional_authorities.iter())
            .collect();
        let programmatic_signer = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_interface::id(),
            &self.authority.pubkey(),
        );

        let executor_address = self
            .executor
            .as_ref()
            .map(|(address, _)| *address)
            .unwrap_or_else(solana_system_interface::program::id);
        let mut executor_instruction = transfer(
            &programmatic_signer,
            &self.recipient,
            DEFAULT_TRANSFER_LAMPORTS,
        );
        executor_instruction.program_id = executor_address;

        let signed_data = executor_instruction.data.clone();

        let signed_payload = SubmitPayload {
            signer_program_id: self
                .signer_program_id
                .unwrap_or(spl_ed25519_signer_interface::id()),
            executor_program_id: self.executor_program_id.unwrap_or(executor_address),
            executor_instruction_data: signed_data.clone(),
        };

        let signatures = self.assemble_signatures(&signers, &signed_payload);
        let authority_pubkeys: Vec<Address> =
            signers.iter().map(|signer| signer.pubkey()).collect();

        let envelope = SubmitEnvelope {
            signatures,
            payload: SubmitPayload {
                executor_instruction_data: self
                    .tampered_executor_data
                    .clone()
                    .unwrap_or(signed_data),
                ..signed_payload.clone()
            },
        };
        let mut instruction = submit(envelope, &authority_pubkeys, &executor_instruction.accounts);

        // The client derives the executor account meta from the payload, so the
        // payload/account mismatch must be recreated by hand.
        if self.executor_program_id.is_some() {
            instruction.accounts[authority_pubkeys.len()].pubkey = executor_address;
        }

        if self.authority_accounts_only {
            instruction.accounts.truncate(authority_pubkeys.len());
        }

        let accounts = self.assemble_accounts(&signers, programmatic_signer);
        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }
        let raw =
            init_mollusk().process_and_validate_instruction(&instruction, &accounts, &self.checks);

        SubmitResult {
            programmatic_signer,
            recipient: self.recipient,
            raw,
        }
    }

    fn assemble_signatures(
        &self,
        signers: &[&Keypair],
        signed_payload: &SubmitPayload,
    ) -> Vec<Signature> {
        if let Some(signatures) = &self.signatures_override {
            return signatures.clone();
        }
        // Each authority signs its own slot, unless a `signed_by` override substitutes
        // a different key.
        let effective: Vec<&dyn Signer> = signers
            .iter()
            .enumerate()
            .map(|(index, default_signer)| {
                self.sign_overrides
                    .iter()
                    .find(|(slot, _)| *slot == index)
                    .map_or(*default_signer, |(_, key)| key) as &dyn Signer
            })
            .collect();

        effective
            .try_sign_message(&signed_payload.signing_bytes().unwrap())
            .unwrap()
    }

    fn assemble_accounts(
        &self,
        signers: &[&Keypair],
        programmatic_signer: Address,
    ) -> Vec<(Address, Account)> {
        let mut accounts: Vec<(Address, Account)> = signers
            .iter()
            .map(|signer| (signer.pubkey(), Account::default()))
            .collect();
        accounts.push(
            self.executor
                .clone()
                .unwrap_or_else(mollusk_svm::program::keyed_account_for_system_program),
        );
        accounts.push((
            programmatic_signer,
            Account {
                lamports: DEFAULT_PDA_LAMPORTS,
                ..Account::default()
            },
        ));
        accounts.push((self.recipient, Account::default()));
        accounts
    }
}

pub struct SubmitResult {
    pub programmatic_signer: Address,
    pub recipient: Address,
    pub raw: InstructionResult,
}

impl SubmitResult {
    pub fn account(&self, key: &Address) -> Option<&Account> {
        self.raw.get_account(key)
    }
}
