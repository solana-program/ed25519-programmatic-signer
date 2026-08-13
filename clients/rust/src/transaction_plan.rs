//! Transaction plan model for cold-signed transaction construction.

use {
    crate::{Error, Result},
    solana_address::Address,
    solana_instruction::Instruction,
    solana_system_interface::instruction as system_instruction,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
};

/// A replay transaction plan that can be wrapped into a programmatic signer transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionPlan {
    /// Instructions replayed by the message executor.
    pub instructions: Vec<Instruction>,
    /// Cold authorities that must sign the cold-signed transaction.
    pub authorities: Vec<Address>,
    /// Live submit signers that must sign the cold-signed transaction and the hot relay transaction.
    pub submit_signers: Vec<Address>,
    /// Nonce account consumed by the executor.
    pub nonce_account: Address,
}

impl TransactionPlan {
    /// Creates a transaction plan from arbitrary replay instructions.
    pub fn new(
        instructions: Vec<Instruction>,
        authorities: Vec<Address>,
        submit_signers: Vec<Address>,
        nonce_account: Address,
    ) -> Result<Self> {
        validate_signers(&authorities, &submit_signers)?;

        Ok(Self {
            instructions,
            authorities,
            submit_signers,
            nonce_account,
        })
    }

    /// Creates a system transfer from an authority's programmatic signer.
    pub fn transfer(
        nonce_account: Address,
        authority: Address,
        recipient: Address,
        lamports: u64,
    ) -> Result<Self> {
        let programmatic_signer =
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), &authority);
        Self::new(
            vec![system_instruction::transfer(
                &programmatic_signer,
                &recipient,
                lamports,
            )],
            vec![authority],
            Vec::new(),
            nonce_account,
        )
    }

    /// Creates an empty transaction plan used to consume a nonce account.
    pub fn cancellation(nonce_account: Address, authority: Address) -> Result<Self> {
        Self::new(Vec::new(), vec![authority], Vec::new(), nonce_account)
    }
}

fn validate_signers(authorities: &[Address], submit_signers: &[Address]) -> Result<()> {
    if authorities.is_empty() {
        return Err(Error::EmptyAuthorities);
    }

    let mut signers = Vec::with_capacity(authorities.len().saturating_add(submit_signers.len()));
    for signer in authorities.iter().chain(submit_signers) {
        if signers.contains(signer) {
            return Err(Error::DuplicateAddress(*signer));
        }
        signers.push(*signer);
    }

    Ok(())
}
