use {
    crate::helpers::common::{
        decode_state, init_mollusk, initialize_durable_signer, signer_account,
        writable_system_account,
    },
    mollusk_svm::{
        Mollusk,
        result::{Check, types::TransactionResult},
    },
    solana_account::Account,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_message::{
        MessageHeader,
        compiled_instruction::CompiledInstruction,
        v1::{Message as MessageV1, TransactionConfig},
    },
    solana_sdk_ids::bpf_loader_upgradeable,
    solana_signer::Signer,
    solana_transaction::{
        AccountMeta as WrappedAccountMeta, Address as WrappedAddress, Hash as WrappedHash,
        Instruction as WrappedInstruction, Signature as WrappedSignature, VersionedMessage,
        versioned::VersionedTransaction,
    },
    spl_ed25519_durable_signer_interface::{
        instruction::DurableSignerInstruction, pda::DurableSignerPda,
    },
};

const DEFAULT_DURABLE_SIGNER_PDA_LAMPORTS: u64 = 100_000_000;

pub struct SubmitBuilder<'a> {
    mollusk: Mollusk,
    durable_signer: Option<(Address, Account)>,
    authority: Keypair,
    additional_authorities: Vec<Keypair>,
    inner_instructions: Vec<WrappedInstruction>,
    lifetime_specifier: Option<Hash>,
    durable_signer_pda_lamports: u64,
    accounts: Vec<(Address, Account)>,
    pre_outer_instructions: Vec<Instruction>,
    post_outer_instructions: Vec<Instruction>,
    submit_instruction: Option<Instruction>,
    zero_signature_indexes: Vec<usize>,
    checks: Vec<Check<'a>>,
}

impl SubmitBuilder<'_> {
    pub fn new(authority: Keypair) -> Self {
        Self {
            mollusk: init_mollusk(),
            durable_signer: None,
            authority,
            additional_authorities: vec![],
            inner_instructions: vec![],
            lifetime_specifier: None,
            durable_signer_pda_lamports: DEFAULT_DURABLE_SIGNER_PDA_LAMPORTS,
            accounts: vec![],
            pre_outer_instructions: vec![],
            post_outer_instructions: vec![],
            submit_instruction: None,
            zero_signature_indexes: vec![],
            checks: vec![],
        }
    }
}

impl<'a> SubmitBuilder<'a> {
    pub fn mollusk(mut self, mollusk: Mollusk) -> Self {
        self.mollusk = mollusk;
        self
    }

    pub fn durable_signer(mut self, durable_signer: (Address, Account)) -> Self {
        self.durable_signer = Some(durable_signer);
        self
    }

    pub fn additional_authority(mut self, authority: Keypair) -> Self {
        self.additional_authorities.push(authority);
        self
    }

    pub fn inner_instruction(mut self, instruction: Instruction) -> Self {
        self.inner_instructions
            .push(wrapped_instruction(instruction));
        self
    }

    pub fn lifetime_specifier(mut self, lifetime_specifier: Hash) -> Self {
        self.lifetime_specifier = Some(lifetime_specifier);
        self
    }

    pub fn durable_signer_pda_lamports(mut self, lamports: u64) -> Self {
        self.durable_signer_pda_lamports = lamports;
        self
    }

    pub fn account(mut self, key: Address, account: Account) -> Self {
        self.accounts.push((key, account));
        self
    }

    pub fn pre_outer_instruction(mut self, instruction: Instruction) -> Self {
        self.pre_outer_instructions.push(instruction);
        self
    }

    pub fn post_outer_instruction(mut self, instruction: Instruction) -> Self {
        self.post_outer_instructions.push(instruction);
        self
    }

    pub fn submit_instruction(mut self, instruction: Instruction) -> Self {
        self.submit_instruction = Some(instruction);
        self
    }

    pub fn zero_signature_at(mut self, index: usize) -> Self {
        self.zero_signature_indexes.push(index);
        self
    }

    pub fn check(mut self, check: Check<'a>) -> Self {
        self.checks.push(check);
        self
    }

    pub fn execute(mut self) -> SubmitResult {
        let program_id = spl_ed25519_durable_signer_interface::id();
        let authority_address = self.authority.pubkey();
        let durable_signer = self
            .durable_signer
            .take()
            .unwrap_or_else(|| initialize_durable_signer(&self.mollusk, &authority_address));
        let state = decode_state(&durable_signer.1);
        let lifetime_specifier = wrapped_hash(self.lifetime_specifier.unwrap_or(state.nonce));

        let authority_pda = DurableSignerPda::derive_address(&program_id, &authority_address);
        let message = MessageV1::try_compile(
            &wrapped_address(authority_pda),
            &self.inner_instructions,
            lifetime_specifier,
        )
        .expect("wrapped message should compile");
        let signed = self.sign_message(&message);
        let submit_instruction = self.submit_instruction.take().unwrap_or_else(|| {
            submit_instruction_for_v1_message(program_id, durable_signer.0, &message, &signed)
        });

        let mut chain = Vec::with_capacity(
            self.pre_outer_instructions
                .len()
                .saturating_add(1)
                .saturating_add(self.post_outer_instructions.len()),
        );
        chain.extend(self.pre_outer_instructions.clone());
        chain.push(submit_instruction.clone());
        chain.extend(self.post_outer_instructions.clone());

        let accounts = self.assemble_accounts(&chain, durable_signer.clone(), &message);
        if self.checks.is_empty() {
            self.checks.push(Check::success());
        }
        let raw = self.mollusk.process_and_validate_transaction_instructions(
            &chain,
            &accounts,
            &self.checks,
        );
        let durable_signer = (
            durable_signer.0,
            raw.get_account(&durable_signer.0)
                .unwrap_or(&durable_signer.1)
                .clone(),
        );

        SubmitResult {
            durable_signer,
            authority_pda,
            message,
            submit_instruction,
            raw,
        }
    }

    fn sign_message(&self, message: &MessageV1) -> SignedV1Message {
        let message_bytes = v1_message_bytes(message);
        let signer_count = usize::from(message.header.num_required_signatures);
        let mut signatures = vec![WrappedSignature::default(); signer_count];
        let mut authorities = vec![Address::default(); signer_count];
        let mut signers = Vec::with_capacity(self.additional_authorities.len().saturating_add(1));
        signers.push(&self.authority);
        for authority in &self.additional_authorities {
            signers.push(authority);
        }

        for signer in signers {
            let authority = signer.pubkey();
            let pda = DurableSignerPda::derive_address(
                &spl_ed25519_durable_signer_interface::id(),
                &authority,
            );
            let signer_index = message.account_keys[..signer_count]
                .iter()
                .position(|key| key.as_array() == pda.as_array())
                .expect("authority PDA must be in the required-signer prefix");
            signatures[signer_index] =
                wrapped_signature(signer.try_sign_message(&message_bytes).unwrap());
            authorities[signer_index] = authority;
        }

        for index in &self.zero_signature_indexes {
            signatures[*index] = WrappedSignature::default();
        }

        SignedV1Message {
            message_bytes,
            signatures,
            authorities,
        }
    }

    fn assemble_accounts(
        &self,
        chain: &[Instruction],
        durable_signer: (Address, Account),
        message: &MessageV1,
    ) -> Vec<(Address, Account)> {
        let mut accounts = vec![durable_signer];
        let mut push_unique = |key: Address, account: Account| {
            if key == solana_sdk_ids::sysvar::instructions::ID {
                return;
            }
            if accounts.iter().any(|(candidate, _)| candidate == &key) {
                return;
            }
            accounts.push((key, account));
        };

        push_unique(
            solana_sdk_ids::sysvar::slot_hashes::ID,
            self.mollusk
                .sysvars
                .keyed_account_for_slot_hashes_sysvar()
                .1,
        );
        push_unique(self.authority.pubkey(), signer_account(0));
        for authority in &self.additional_authorities {
            push_unique(authority.pubkey(), signer_account(0));
        }
        push_unique(
            solana_system_interface::program::id(),
            mollusk_svm::program::keyed_account_for_system_program().1,
        );
        let authority_pda = DurableSignerPda::derive_address(
            &spl_ed25519_durable_signer_interface::id(),
            &self.authority.pubkey(),
        );
        push_unique(
            authority_pda,
            writable_system_account(self.durable_signer_pda_lamports),
        );
        for (key, account) in &self.accounts {
            push_unique(*key, account.clone());
        }
        for key in &message.account_keys {
            push_unique(unwrapped_address(*key), writable_system_account(0));
        }
        for instruction in chain {
            for meta in &instruction.accounts {
                push_unique(meta.pubkey, writable_system_account(0));
            }
        }

        accounts
    }
}

pub struct SignedV1Message {
    pub message_bytes: Vec<u8>,
    pub signatures: Vec<WrappedSignature>,
    pub authorities: Vec<Address>,
}

pub struct SubmitResult {
    pub durable_signer: (Address, Account),
    pub authority_pda: Address,
    pub message: MessageV1,
    pub submit_instruction: Instruction,
    pub raw: TransactionResult,
}

impl SubmitResult {
    pub fn account(&self, key: &Address) -> Option<&Account> {
        self.raw.get_account(key)
    }
}

pub fn submit_instruction_for_v1_message(
    program_id: Address,
    durable_signer: Address,
    message: &MessageV1,
    signed: &SignedV1Message,
) -> Instruction {
    let transaction = VersionedTransaction {
        signatures: signed.signatures.clone(),
        message: VersionedMessage::V1(message.clone()),
    };
    let data = wincode::serialize(&DurableSignerInstruction::Submit(transaction)).unwrap();

    let mut accounts = Vec::with_capacity(
        3usize
            .saturating_add(signed.authorities.len())
            .saturating_add(message.account_keys.len()),
    );
    accounts.push(AccountMeta::new(durable_signer, false));
    accounts.push(AccountMeta::new_readonly(
        solana_sdk_ids::sysvar::slot_hashes::ID,
        false,
    ));
    accounts.push(AccountMeta::new_readonly(
        solana_sdk_ids::sysvar::instructions::ID,
        false,
    ));
    for authority in &signed.authorities {
        accounts.push(AccountMeta::new_readonly(*authority, false));
    }
    for (index, key) in message.account_keys.iter().enumerate() {
        accounts.push(if is_v1_maybe_writable(message, index) {
            AccountMeta::new(unwrapped_address(*key), false)
        } else {
            AccountMeta::new_readonly(unwrapped_address(*key), false)
        });
    }

    Instruction {
        program_id,
        accounts,
        data,
    }
}

pub fn empty_v1_message(lifetime_specifier: Hash, signer: Address) -> MessageV1 {
    MessageV1 {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        config: TransactionConfig::empty(),
        lifetime_specifier: wrapped_hash(lifetime_specifier),
        account_keys: vec![wrapped_address(signer)],
        instructions: vec![],
    }
}

pub fn wrapped_hash(hash: Hash) -> WrappedHash {
    WrappedHash::new_from_array(*hash.as_bytes())
}

pub fn wrapped_signature(signature: solana_signature::Signature) -> WrappedSignature {
    WrappedSignature::from(*signature.as_array())
}

fn wrapped_instruction(instruction: Instruction) -> WrappedInstruction {
    WrappedInstruction {
        program_id: wrapped_address(instruction.program_id),
        accounts: instruction
            .accounts
            .into_iter()
            .map(|meta| WrappedAccountMeta {
                pubkey: wrapped_address(meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: instruction.data,
    }
}

pub fn compiled_transfer_instruction(
    from_index: u8,
    to_index: u8,
    program_id_index: u8,
    lamports: u64,
) -> CompiledInstruction {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    CompiledInstruction {
        program_id_index,
        accounts: vec![from_index, to_index],
        data,
    }
}

pub fn v1_message_bytes(message: &MessageV1) -> Vec<u8> {
    VersionedMessage::V1(message.clone()).serialize()
}

fn is_v1_maybe_writable(message: &MessageV1, index: usize) -> bool {
    let required_signatures = usize::from(message.header.num_required_signatures);
    let readonly_signed = usize::from(message.header.num_readonly_signed_accounts);
    let readonly_unsigned = usize::from(message.header.num_readonly_unsigned_accounts);
    let requested_writable = index < required_signatures.saturating_sub(readonly_signed)
        || (index >= required_signatures
            && index < message.account_keys.len().saturating_sub(readonly_unsigned));

    requested_writable
        && (!is_key_called_as_program(&message.instructions, index)
            || message
                .account_keys
                .iter()
                .any(|key| key.as_array() == bpf_loader_upgradeable::ID.as_array()))
}

fn is_key_called_as_program(instructions: &[CompiledInstruction], key_index: usize) -> bool {
    let Ok(key_index) = u8::try_from(key_index) else {
        return false;
    };
    instructions
        .iter()
        .any(|instruction| instruction.program_id_index == key_index)
}

pub fn wrapped_address(address: Address) -> WrappedAddress {
    WrappedAddress::new_from_array(address.to_bytes())
}

pub fn unwrapped_address(address: WrappedAddress) -> Address {
    Address::new_from_array(address.to_bytes())
}
