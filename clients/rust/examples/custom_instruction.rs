//! Compose a custom Memo instruction with a required programmatic signer.
use {
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_signer::Signer,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_programmatic_signer_client::{
        TransactionPlan, build_transaction, inspect, sign_transaction,
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cold = Keypair::new();
    let pda = ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), &cold.pubkey());
    let instruction = Instruction {
        program_id: "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr".parse()?,
        accounts: vec![AccountMeta::new_readonly(pda, true)],
        data: b"Reviewed offline".to_vec(),
    };
    let plan = TransactionPlan::new(
        vec![instruction],
        vec![cold.pubkey()],
        vec![],
        Address::new_unique(),
    )?;
    // Supply the actual nonce value and cluster genesis hash in a real application.
    let mut transaction = build_transaction(
        &plan,
        Hash::new_from_array([2; 32]),
        Hash::new_from_array([1; 32]),
    )?;
    sign_transaction(&mut transaction, &cold)?;
    let summary = inspect(&transaction)?;
    println!("Signed custom instruction for PDA {pda}");
    println!("Inner instructions: {}", summary.inner_instructions.len());
    Ok(())
}
