//! End-to-end flow where a cold wallet authorizes a wrapped message offline. The hot wallet
//! submits it through the Ed25519 Signer program, which CPIs into the Message Executor and
//! replays the message with the cold wallet's programmatic signer promoted.

use {
    crate::helpers::common::{
        decode_state, init_mollusk, keyed_account_for_nonce_program, message_hash, system_account,
        system_transfer_instruction,
    },
    mollusk_svm::{Mollusk, result::Check},
    solana_account::Account,
    solana_address::Address,
    solana_instruction::Instruction,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_program_error::ProgramError,
    solana_signer::Signer as _,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_client::{instruction::submit, message::wrapped_message},
    spl_ed25519_signer_interface::{error::Error as SignerError, pda::ProgrammaticSigner},
    spl_message_executor_client::instruction::execute,
    spl_message_executor_interface::error::Error as MessageExecutorError,
    spl_nonce_client::instruction::initialize,
};

pub mod helpers;

const PDA_LAMPORTS: u64 = 100_000_000;
const TRANSFER_LAMPORTS: u64 = 1_000_000;

struct ColdWalletFlow {
    mollusk: Mollusk,
    authority: Keypair,
    programmatic_signer: Address,
    nonce_account: (Address, Account),
    recipient: Address,
}

impl ColdWalletFlow {
    fn new() -> Self {
        let mut mollusk = init_mollusk();
        mollusk.add_program(
            &spl_ed25519_signer_interface::id(),
            "spl_ed25519_signer_program",
        );

        let authority = Keypair::new();
        let programmatic_signer = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_interface::id(),
            &authority.pubkey(),
        );

        // The nonce account's authority is the programmatic signer. Consuming the nonce
        // requires that PDA's signer privilege, which only a valid wrapped transaction signature
        // can produce.
        let nonce_account_address = Address::new_unique();
        let nonce_account = {
            let instruction = initialize(&nonce_account_address, &programmatic_signer);
            let result = mollusk.process_and_validate_instruction(
                &instruction,
                &[
                    (
                        nonce_account_address,
                        crate::helpers::nonce_account_builder::NonceAccountBuilder::new()
                            .key(nonce_account_address)
                            .build()
                            .1,
                    ),
                    (programmatic_signer, system_account(0)),
                    mollusk.sysvars.keyed_account_for_slot_hashes_sysvar(),
                ],
                &[Check::success()],
            );
            (
                nonce_account_address,
                result.get_account(&nonce_account_address).unwrap().clone(),
            )
        };

        Self {
            mollusk,
            authority,
            programmatic_signer,
            nonce_account,
            recipient: Address::new_unique(),
        }
    }

    /// The wrapped message the cold wallet wants executed. It transfers from the
    /// programmatic signer and spends the current nonce.
    fn message(&self) -> VersionedMessage {
        VersionedMessage::Legacy(Message::new_with_blockhash(
            &[crate::helpers::execute_builder::message_instruction(
                system_transfer_instruction(
                    self.programmatic_signer,
                    self.recipient,
                    TRANSFER_LAMPORTS,
                ),
            )],
            Some(&self.programmatic_signer),
            &message_hash(decode_state(&self.nonce_account.1).nonce),
        ))
    }

    /// Builds the outer `Submit` instruction for a message, signed by the cold wallet.
    fn submit_instruction(&self, message: &VersionedMessage) -> Instruction {
        let executor_instruction = execute(&self.nonce_account.0, message);
        let wrapped = wrapped_message(&executor_instruction, &[self.authority.pubkey()]);
        let transaction = VersionedTransaction::try_new(wrapped, &[&self.authority]).unwrap();
        submit(transaction)
    }

    fn accounts(&self, instruction: &Instruction) -> Vec<(Address, Account)> {
        instruction
            .accounts
            .iter()
            .map(|meta| self.account_for(meta.pubkey))
            .collect()
    }

    fn account_for(&self, key: Address) -> (Address, Account) {
        if key == self.authority.pubkey() {
            return (key, system_account(0));
        }
        if key == spl_message_executor_interface::id() {
            return (
                key,
                mollusk_svm::program::create_program_account_loader_v3(
                    &spl_message_executor_interface::id(),
                ),
            );
        }
        if key == spl_nonce_interface::id() {
            return keyed_account_for_nonce_program();
        }
        if key == self.nonce_account.0 {
            return self.nonce_account.clone();
        }
        if key == solana_sdk_ids::sysvar::slot_hashes::id() {
            return self.mollusk.sysvars.keyed_account_for_slot_hashes_sysvar();
        }
        if key == self.programmatic_signer {
            return (key, system_account(PDA_LAMPORTS));
        }
        if key == self.recipient {
            return (key, system_account(0));
        }
        if key == solana_system_interface::program::id() {
            return mollusk_svm::program::keyed_account_for_system_program();
        }
        (key, Account::default())
    }
}

#[test]
fn cold_wallet_flow_executes_message_and_advances_nonce() {
    let flow = ColdWalletFlow::new();
    let old_nonce = decode_state(&flow.nonce_account.1).nonce;
    let instruction = flow.submit_instruction(&flow.message());

    let result = flow.mollusk.process_and_validate_instruction(
        &instruction,
        &flow.accounts(&instruction),
        &[Check::success()],
    );

    assert_eq!(
        result.get_account(&flow.recipient).unwrap().lamports,
        TRANSFER_LAMPORTS
    );
    assert_eq!(
        result
            .get_account(&flow.programmatic_signer)
            .unwrap()
            .lamports,
        PDA_LAMPORTS - TRANSFER_LAMPORTS
    );
    assert_ne!(
        decode_state(result.get_account(&flow.nonce_account.0).unwrap()).nonce,
        old_nonce
    );
}

#[test]
fn cold_wallet_flow_executes_batched_message() {
    // Two transfers replayed from one signed wrapped message through the full stack.
    let flow = ColdWalletFlow::new();
    let message = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[
            crate::helpers::execute_builder::message_instruction(system_transfer_instruction(
                flow.programmatic_signer,
                flow.recipient,
                TRANSFER_LAMPORTS,
            )),
            crate::helpers::execute_builder::message_instruction(system_transfer_instruction(
                flow.programmatic_signer,
                flow.recipient,
                2 * TRANSFER_LAMPORTS,
            )),
        ],
        Some(&flow.programmatic_signer),
        &message_hash(decode_state(&flow.nonce_account.1).nonce),
    ));
    let instruction = flow.submit_instruction(&message);

    let result = flow.mollusk.process_and_validate_instruction(
        &instruction,
        &flow.accounts(&instruction),
        &[Check::success()],
    );

    assert_eq!(
        result.get_account(&flow.recipient).unwrap().lamports,
        3 * TRANSFER_LAMPORTS
    );
}

#[test]
fn cold_wallet_flow_rejects_wrapped_transaction_replay() {
    let mut flow = ColdWalletFlow::new();
    let message = flow.message();
    let instruction = flow.submit_instruction(&message);

    let first = flow.mollusk.process_and_validate_instruction(
        &instruction,
        &flow.accounts(&instruction),
        &[Check::success()],
    );

    // Replaying the identical signed wrapped transaction must fail because the nonce advanced.
    flow.nonce_account.1 = first.get_account(&flow.nonce_account.0).unwrap().clone();
    flow.mollusk.process_and_validate_instruction(
        &instruction,
        &flow.accounts(&instruction),
        &[Check::err(ProgramError::Custom(
            MessageExecutorError::NonceMismatch as u32,
        ))],
    );
}

#[test]
fn cold_wallet_flow_rejects_tampered_message() {
    let flow = ColdWalletFlow::new();

    // The cold wallet signed for the intended executor instruction...
    let executor_instruction = execute(&flow.nonce_account.0, &flow.message());
    let wrapped = wrapped_message(&executor_instruction, &[flow.authority.pubkey()]);
    let mut transaction = VersionedTransaction::try_new(wrapped, &[&flow.authority]).unwrap();

    // ...but the hot wallet mutates the signed wrapped message to drain the full balance.
    let drain = VersionedMessage::Legacy(Message::new_with_blockhash(
        &[crate::helpers::execute_builder::message_instruction(
            system_transfer_instruction(flow.programmatic_signer, flow.recipient, PDA_LAMPORTS),
        )],
        Some(&flow.programmatic_signer),
        &message_hash(decode_state(&flow.nonce_account.1).nonce),
    ));
    let drain_instruction = execute(&flow.nonce_account.0, &drain);
    let VersionedMessage::Legacy(wrapped_message) = &mut transaction.message else {
        panic!("expected legacy wrapped message");
    };
    wrapped_message.instructions[0].data = drain_instruction.data;
    let tampered = submit(transaction);

    flow.mollusk.process_and_validate_instruction(
        &tampered,
        &flow.accounts(&tampered),
        &[Check::err(SignerError::InvalidSignature.into())],
    );
}
