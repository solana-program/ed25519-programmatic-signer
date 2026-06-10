//! Builder for the on-chain `Submit` instruction.

use {
    alloc::{vec, vec::Vec},
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    spl_ed25519_signer_interface::{
        instruction::{Instruction as SignerInstruction, SubmitEnvelope},
        pda::ProgrammaticSigner,
    },
};

/// Builds the `Submit` instruction from a signed envelope.
///
/// `authorities` must pair by index with `envelope.signatures`, and `executor_accounts`
/// are the executor instruction's account metas, in the order it expects. The metas are
/// forwarded with signer flags cleared on each authority's `ProgrammaticSigner`. The program
/// promotes those during CPI.
pub fn submit(
    envelope: SubmitEnvelope,
    authorities: &[Address],
    executor_accounts: &[AccountMeta],
) -> Instruction {
    let program_id = spl_ed25519_signer_interface::id();
    let programmatic_signers: Vec<Address> = authorities
        .iter()
        .map(|authority| ProgrammaticSigner::derive_address(&program_id, authority))
        .collect();

    let mut accounts = vec![];
    for authority in authorities {
        accounts.push(AccountMeta::new_readonly(*authority, false));
    }
    accounts.push(AccountMeta::new_readonly(
        envelope.payload.executor_program_id,
        false,
    ));
    for meta in executor_accounts {
        let is_promoted = programmatic_signers.contains(&meta.pubkey);
        accounts.push(AccountMeta {
            pubkey: meta.pubkey,
            // A programmatic signer is a PDA, so no transaction signature can exist for
            // it. Clear its signer flag here, the program signs for it during the CPI.
            is_signer: meta.is_signer && !is_promoted,
            is_writable: meta.is_writable,
        });
    }

    Instruction::new_with_wincode(program_id, &SignerInstruction::Submit(envelope), accounts)
}
