use {
    crate::{
        artifact,
        output::{Account, Inspection, Instruction, OutputFormat, SignerStatus},
    },
    anyhow::Result,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_address::Address,
    solana_system_interface::{instruction::SystemInstruction, program as system_program},
    spl_programmatic_signer_client::{inspect, nonce::next_nonce},
    spl_token_interface::instruction::TokenInstruction,
    std::{collections::BTreeSet, path::Path},
};

pub(super) fn run(path: &Path, output: OutputFormat) -> Result<String> {
    let transaction = artifact::read(path)?;
    let summary = inspect(&transaction)?;
    let instructions = summary
        .inner_instructions
        .iter()
        .map(|instruction| {
            // inspect() validates every compiled index before presentation.
            let program = summary.inner_account_keys[usize::from(instruction.program_id_index)];
            let accounts = instruction
                .accounts
                .iter()
                .map(|index| summary.inner_account_keys[usize::from(*index)].to_string())
                .collect::<Vec<_>>();
            Instruction {
                program_id: program.to_string(),
                description: describe(&program, &accounts, &instruction.data),
                accounts,
                data_base64: STANDARD.encode(&instruction.data),
            }
        })
        .collect();
    let inner_accounts = summary
        .inner_account_keys
        .iter()
        .enumerate()
        .map(|(index, key)| Account {
            address: key.to_string(),
            is_signer: summary.inner_message.is_signer(index),
            is_writable: summary
                .inner_message
                .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
        })
        .collect();
    output.render(&Inspection {
        genesis_hash: summary.genesis_hash.to_string(),
        signer_program: spl_ed25519_signer_client::id().to_string(),
        executor_program: summary.executor_program.to_string(),
        nonce_program: spl_nonce_interface::id().to_string(),
        nonce_account: summary.nonce_account.to_string(),
        expected_nonce: summary.inner_message.recent_blockhash().to_string(),
        next_nonce: next_nonce(&transaction)?.to_string(),
        transaction_signers: summary
            .wrapper_signers
            .iter()
            .map(|signer| SignerStatus {
                address: signer.address.to_string(),
                signed: signer.signed,
            })
            .collect(),
        inner_accounts,
        inner_instructions: instructions,
    })
}

fn describe(program: &Address, accounts: &[String], data: &[u8]) -> String {
    if *program == system_program::ID {
        if let Ok(SystemInstruction::Transfer { lamports }) =
            wincode::deserialize_exact::<SystemInstruction>(data)
        {
            if let [from, to, ..] = accounts {
                return format!("Transfer {lamports} lamports from {from} to {to}");
            }
        }
    }
    if *program == spl_token_interface::id() {
        match TokenInstruction::unpack(data) {
            Ok(TokenInstruction::Transfer { amount }) => {
                if let [from, to, authority, ..] = accounts {
                    return format!(
                        "Transfer {amount} raw token units from {from} to {to}, authority \
                         {authority}; mint and decimals are not encoded"
                    );
                }
            }
            Ok(TokenInstruction::TransferChecked { amount, decimals }) => {
                if let [from, mint, to, authority, ..] = accounts {
                    return format!(
                        "Transfer {amount} raw token units ({decimals} decimals), mint {mint}, \
                         from {from} to {to}, authority {authority}"
                    );
                }
            }
            _ => {}
        }
    }
    if program.to_string() == "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr" {
        if let Ok(text) = std::str::from_utf8(data) {
            return format!("Memo {text:?}");
        }
    }
    "Undecoded instruction; review the program, accounts, and raw data".into()
}
