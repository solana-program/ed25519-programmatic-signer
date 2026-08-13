//! Nonce account construction and decoding helpers.

use {
    crate::{Error, Result, TransactionPlan, build_transaction},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_system_interface::instruction as system_instruction,
    spl_nonce_client::instruction::{advance, initialize},
    spl_nonce_interface::state::Nonce,
};

/// Builds the create-account and initialize instructions for a nonce account.
///
/// `authority` is stored as the nonce account authority verbatim. Wrapped transactions
/// through the signer program need the `ProgrammaticSigner` PDA here. Direct-advance
/// nonce accounts need a keypair address.
pub fn create_nonce_account_instructions(
    payer: &Address,
    nonce_account: &Address,
    authority: &Address,
    rent_lamports: u64,
) -> [Instruction; 2] {
    [
        system_instruction::create_account(
            payer,
            nonce_account,
            rent_lamports,
            Nonce::LEN as u64,
            &spl_nonce_interface::id(),
        ),
        initialize(nonce_account, authority),
    ]
}

/// Decodes nonce account data.
pub fn decode(account_data: &[u8]) -> Result<Nonce> {
    wincode::deserialize_exact(account_data).map_err(|_| Error::InvalidNonceAccount)
}

/// Builds an `Advance` instruction for the current nonce account value.
pub fn advance_instruction(
    nonce_account: &Address,
    authority: &Address,
    current_nonce: Hash,
) -> Instruction {
    advance(authority, nonce_account, current_nonce)
}

/// Builds an empty cold-signed transaction that only consumes a nonce account.
///
/// Use this when the nonce account authority is the authority's ProgrammaticSigner PDA.
/// Keypair-authority nonce accounts can use [`advance_instruction`] directly.
pub fn advance_transaction(
    nonce_account: Address,
    authority: Address,
    current_nonce: Hash,
    genesis_hash: Hash,
) -> Result<solana_transaction::versioned::VersionedTransaction> {
    build_transaction(
        &TransactionPlan::cancellation(nonce_account, authority)?,
        current_nonce,
        genesis_hash,
    )
}

#[cfg(test)]
mod tests {
    use {super::advance_instruction, solana_address::Address, solana_hash::Hash};

    #[test]
    fn advance_instruction_orders_authority_before_nonce_account() {
        let nonce_account = Address::new_from_array([1; 32]);
        let authority = Address::new_from_array([2; 32]);

        let instruction = advance_instruction(&nonce_account, &authority, Hash::default());

        assert_eq!(instruction.accounts[0].pubkey, authority);
        assert!(instruction.accounts[0].is_signer);
        assert!(!instruction.accounts[0].is_writable);
        assert_eq!(instruction.accounts[1].pubkey, nonce_account);
        assert!(!instruction.accounts[1].is_signer);
        assert!(instruction.accounts[1].is_writable);
    }
}
