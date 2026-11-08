use {
    pinocchio::{
        AccountView, ProgramResult,
        cpi::{Signer, invoke_signed_with_bounds},
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_hash::Hash,
    spl_nonce_interface::instruction::Instruction,
};

/// Consumes a nonce and commits its successor to a transition signer and commitment.
#[derive(Debug)]
pub struct Advance<'a> {
    /// Authority stored in the nonce account.
    pub authority: &'a AccountView,
    /// Signer whose address is bound into the successor nonce.
    pub transition_signer: &'a AccountView,
    /// Writable nonce account to advance.
    pub nonce_account: &'a AccountView,
    /// Nonce value the account must currently store.
    pub current_nonce: Hash,
    /// Commitment bound into the successor nonce.
    pub transition_commitment: Hash,
}

impl Advance<'_> {
    /// Consumes this instruction and invokes it using existing signer privileges.
    pub fn invoke(self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    /// Consumes this instruction and signs only this CPI with the supplied PDA seeds.
    pub fn invoke_signed(self, signers: &[Signer]) -> ProgramResult {
        let instruction = Instruction::Advance {
            current_nonce: self.current_nonce,
            transition_commitment: self.transition_commitment,
        };
        let data =
            wincode::serialize(&instruction).map_err(|_| ProgramError::InvalidInstructionData)?;
        let instruction_accounts = [
            InstructionAccount::readonly_signer(self.authority.address()),
            InstructionAccount::readonly_signer(self.transition_signer.address()),
            InstructionAccount::writable(self.nonce_account.address()),
        ];
        let view = InstructionView {
            program_id: &spl_nonce_interface::ID,
            accounts: &instruction_accounts,
            data: &data,
        };
        invoke_signed_with_bounds::<3, &AccountView>(
            &view,
            &[self.authority, self.transition_signer, self.nonce_account],
            signers,
        )
    }
}

/// Withdraws lamports from a nonce account, closing it when its full balance is withdrawn.
#[derive(Debug)]
pub struct Withdraw<'a> {
    /// Stored authority, or the nonce account itself if the account is not initialized.
    pub authority: &'a AccountView,
    /// Writable nonce account to withdraw from.
    pub nonce_account: &'a AccountView,
    /// Writable account receiving the lamports.
    pub destination: &'a AccountView,
    /// Amount to withdraw.
    pub lamports: u64,
}

impl Withdraw<'_> {
    /// Consumes this instruction and invokes it using existing signer privileges.
    pub fn invoke(self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    /// Consumes this instruction and signs only this CPI with the supplied PDA seeds.
    pub fn invoke_signed(self, signers: &[Signer]) -> ProgramResult {
        let data = wincode::serialize(&Instruction::Withdraw {
            lamports: self.lamports,
        })
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        let instruction_accounts = [
            InstructionAccount::readonly_signer(self.authority.address()),
            InstructionAccount::writable(self.nonce_account.address()),
            InstructionAccount::writable(self.destination.address()),
        ];
        let view = InstructionView {
            program_id: &spl_nonce_interface::ID,
            accounts: &instruction_accounts,
            data: &data,
        };
        invoke_signed_with_bounds::<3, &AccountView>(
            &view,
            &[self.authority, self.nonce_account, self.destination],
            signers,
        )
    }
}
