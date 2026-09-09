//! Build, sign, verify, and serialize a SOL transfer without RPC.
use {
    base64::{Engine as _, engine::general_purpose::STANDARD},
    solana_address::Address,
    solana_hash::Hash,
    solana_keypair::Keypair,
    solana_signer::Signer,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_client::{
        TransactionPlan, build_transaction, nonce::next_nonce, sign_transaction,
        submit_transaction, verify,
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cold = Keypair::new();
    let hot = Keypair::new();
    let nonce_account = Address::new_unique();
    let recipient = Address::new_unique();
    // These values are synthetic. Fetch and authenticate a real snapshot before use.
    let genesis = Hash::new_from_array([1; 32]);
    let state = Nonce {
        nonce: Hash::new_from_array([2; 32]),
        authority: ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &cold.pubkey(),
        ),
    };
    let plan = TransactionPlan::transfer(nonce_account, cold.pubkey(), recipient, 1_000_000)?;
    let mut transaction = build_transaction(&plan, state.nonce, genesis)?;
    sign_transaction(&mut transaction, &cold)?;
    verify(&transaction, &state, &nonce_account, &genesis)?;
    println!("Conditional next nonce: {}", next_nonce(&transaction)?);
    println!(
        "Signed wrapper (base64): {}",
        STANDARD.encode(wincode::serialize(&transaction)?)
    );
    let relay = submit_transaction(&transaction, &hot, &[], Hash::new_from_array([3; 32]))?;
    println!(
        "Relay size: {} bytes; example did not contact RPC",
        wincode::serialize(&relay)?.len()
    );
    Ok(())
}
