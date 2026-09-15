use {
    base64::{Engine, prelude::BASE64_STANDARD},
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy::Message},
    solana_system_interface::instruction::transfer,
    spl_ed25519_signer_client::{ProgrammaticSigner, message::wrapped_message},
    spl_legacy_message_executor_client::instruction::execute,
};

/// Build an approval fixture containing one `Execute` instruction. The inner message transfers
/// one lamport from each authority's programmatic signer to a fixed recipient.
pub fn approval_message(authorities: &[Address]) -> Message {
    let transfers = authorities
        .iter()
        .map(|authority| {
            let signer =
                ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority);
            transfer(&signer, &Address::new_from_array([3; 32]), 1)
        })
        .collect::<Vec<_>>();
    let inner = Message::new_with_blockhash(&transfers, None, &Hash::new_from_array([8; 32]));
    let instruction = execute(&Address::new_from_array([2; 32]), &inner);
    let VersionedMessage::Legacy(outer) = wrapped_message(&instruction, authorities) else {
        panic!("wrapped_message must produce a legacy message");
    };
    outer
}

pub fn sign_only(msg: &VersionedMessage) -> CliSignOnlyData {
    CliSignOnlyData {
        blockhash: msg.recent_blockhash().to_string(),
        message: Some(BASE64_STANDARD.encode(msg.serialize())),
        ..CliSignOnlyData::default()
    }
}
