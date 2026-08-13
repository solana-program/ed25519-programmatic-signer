use {
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde_json::{Value, json},
    solana_address::Address,
    solana_system_interface::{instruction::SystemInstruction, program as system_program},
};

pub(crate) fn format_transaction_summary(
    summary: &spl_programmatic_signer_rust::TransactionSummary,
) -> String {
    let mut lines = vec![
        format!("genesis hash: {}", summary.genesis_hash),
        format!("executor program: {}", summary.executor_program),
        format!("nonce account: {}", summary.nonce_account),
        format!(
            "expected nonce: {}",
            summary.inner_message.recent_blockhash()
        ),
        String::from("transaction signers:"),
    ];
    for signer in &summary.wrapper_signers {
        lines.push(format!(
            "  {} {}",
            signer.address,
            if signer.signed { "signed" } else { "missing" }
        ));
    }
    lines.push(String::from("inner required signers:"));
    for signer in &summary.inner_required_signers {
        lines.push(format!("  {signer}"));
    }
    lines.push(String::from("inner account keys:"));
    for (index, account) in summary.inner_account_keys.iter().enumerate() {
        lines.push(format!("  [{index}] {account}"));
    }
    lines.push(String::from("inner instructions:"));
    for (index, instruction) in summary.inner_instructions.iter().enumerate() {
        lines.push(format!(
            "  [{index}] {}",
            describe_compiled_instruction(
                instruction.program_id_index,
                &instruction.accounts,
                &instruction.data,
                &summary.inner_account_keys,
            )
        ));
    }
    lines.join("\n")
}

pub(crate) fn transaction_summary_json(
    summary: &spl_programmatic_signer_rust::TransactionSummary,
) -> Value {
    json!({
        "genesisHash": summary.genesis_hash.to_string(),
        "executorProgram": summary.executor_program.to_string(),
        "nonceAccount": summary.nonce_account.to_string(),
        "expectedNonce": summary.inner_message.recent_blockhash().to_string(),
        "transactionSigners": summary.wrapper_signers.iter().map(|status| {
            json!({
                "address": status.address.to_string(),
                "signed": status.signed,
            })
        }).collect::<Vec<_>>(),
        "innerRequiredSigners": summary.inner_required_signers.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "innerAccountKeys": summary.inner_account_keys.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "innerInstructions": summary.inner_instructions.iter().map(|instruction| {
            compiled_instruction_json(
                instruction.program_id_index,
                &instruction.accounts,
                &instruction.data,
                &summary.inner_account_keys,
            )
        }).collect::<Vec<_>>(),
    })
}

fn compiled_instruction_json(
    program_id_index: u8,
    account_indexes: &[u8],
    data: &[u8],
    account_keys: &[Address],
) -> Value {
    let program_id = resolve_account(account_keys, program_id_index).map(ToString::to_string);
    json!({
        "programIdIndex": program_id_index,
        "programId": program_id,
        "accounts": account_indexes.iter().map(|index| {
            json!({
                "index": index,
                "address": resolve_account(account_keys, *index).map(ToString::to_string),
            })
        }).collect::<Vec<_>>(),
        "dataBase64": STANDARD.encode(data),
        "description": describe_compiled_instruction(program_id_index, account_indexes, data, account_keys),
    })
}

fn describe_compiled_instruction(
    program_id_index: u8,
    account_indexes: &[u8],
    data: &[u8],
    account_keys: &[Address],
) -> String {
    let Some(program_id) = resolve_account(account_keys, program_id_index) else {
        return format!(
            "unknown program index {program_id_index}, accounts {:?}, data {}",
            account_indexes,
            STANDARD.encode(data)
        );
    };
    if *program_id == system_program::ID {
        return describe_system_instruction(account_indexes, data, account_keys);
    }
    format!(
        "program {}, accounts {:?}, data {}",
        program_id,
        account_indexes,
        STANDARD.encode(data)
    )
}

fn describe_system_instruction(
    account_indexes: &[u8],
    data: &[u8],
    account_keys: &[Address],
) -> String {
    let instruction = wincode::deserialize_exact::<SystemInstruction>(data);
    match instruction {
        Ok(SystemInstruction::Transfer { lamports }) => {
            let from = account_indexes
                .first()
                .and_then(|index| resolve_account(account_keys, *index))
                .map(ToString::to_string)
                .unwrap_or_else(|| String::from("<missing>"));
            let to = account_indexes
                .get(1)
                .and_then(|index| resolve_account(account_keys, *index))
                .map(ToString::to_string)
                .unwrap_or_else(|| String::from("<missing>"));
            format!("system transfer {lamports} lamports from {from} to {to}")
        }
        Ok(other) => format!("system {other:?}"),
        Err(_) => format!(
            "system instruction, accounts {:?}, data {}",
            account_indexes,
            STANDARD.encode(data)
        ),
    }
}

fn resolve_account(account_keys: &[Address], index: u8) -> Option<&Address> {
    account_keys.get(usize::from(index))
}
