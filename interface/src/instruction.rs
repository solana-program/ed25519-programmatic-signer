use {
    solana_address::Address,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Nonce program.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonceInstruction {
    /// Creates a nonce account at the PDA derived from a caller-chosen 32-byte `nonce_id`.
    ///
    /// Instruction data: [`InitializeData`].
    ///
    /// On success, the program:
    /// 1. Allocates and assigns the PDA via System program CPI. Caller must pre-fund it with
    ///    rent-exempt lamports.
    /// 2. Derives the initial `nonce` as
    ///    `sha256("spl-nonce::init-v1" ‖ state_pda_address ‖ slot_hashes[0])`.
    /// 3. Writes `NonceState { nonce, authority }` into the account data.
    ///
    /// Accounts required:
    /// - `[writable]` `NonceStatePda`, pre-funded
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` System program
    Initialize,

    /// Authorizes and executes a wrapped Solana transaction signed by `NonceState` authority.
    ///
    /// Instruction data: `solana_transaction::Transaction`.
    ///
    /// On success, the program:
    /// 1. Deserializes the `Transaction` and sanitizes the wrapped message.
    /// 2. Checks `message.account_keys[0] == state.authority`.
    /// 3. Checks `message.recent_blockhash == state.nonce`.
    /// 4. Verifies that every signer declared by the wrapped message either signs the
    ///    outer transaction or the inner wrapped transaction.
    /// 5. Executes each `message.instructions` entry by CPI, promoting `NonceAuthorityPda`
    ///    to signer wherever referenced.
    /// 6. Derives and stores the next nonce as
    ///    `sha256("spl-nonce::v1" ‖ state_pda ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(bincode(tx.message)))`.
    ///
    /// Accounts required:
    /// - `[writable]` `NonceStatePda`
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Remaining: `tx.message.account_keys`, in order, with `is_signer` and `is_writable`
    ///   flags matching the wrapped message.
    Submit,

    /// Rotates the authority controlling this nonce account.
    ///
    /// Instruction data: [`SetAuthorityData`].
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`.
    /// Inherits the authorization from the outer `Submit`. A direct outer call cannot succeed,
    /// because nothing outside this program can sign for `NonceAuthorityPda`.
    ///
    /// Accounts required:
    /// - `[signer]` `NonceAuthorityPda`
    /// - `[writable]` `NonceStatePda`
    SetAuthority,

    /// Closes a nonce account and refunds its lamports.
    ///
    /// Instruction data: [`CloseData`].
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`
    /// for the same reason as `SetAuthority`.
    ///
    /// Accounts required:
    /// - `[signer]` `NonceAuthorityPda`
    /// - `[writable]` `NonceStatePda`
    /// - `[writable]` Lamport recipient
    Close,
}

/// Data for [`NonceInstruction::Initialize`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct InitializeData {
    /// Caller-chosen identifier for this nonce account. Each distinct value derives its own
    /// [`NonceStatePda`](crate::pda::NonceStatePda).
    pub nonce_id: [u8; 32],
    /// Authorizes `Submit` ix for this account.
    pub authority: Address,
}

/// Data for [`NonceInstruction::SetAuthority`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct SetAuthorityData {
    /// Replacement authority address.
    pub authority: Address,
}

/// Data for [`NonceInstruction::Close`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct CloseData {
    /// Address that receives all lamports from the closed nonce account.
    pub recipient: Address,
}

impl TryFrom<u8> for NonceInstruction {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Initialize),
            1 => Ok(Self::Submit),
            2 => Ok(Self::SetAuthority),
            3 => Ok(Self::Close),
            _ => Err(()),
        }
    }
}

impl From<NonceInstruction> for u8 {
    fn from(value: NonceInstruction) -> Self {
        value as u8
    }
}

#[cfg(test)]
mod tests {
    use super::NonceInstruction;

    #[test]
    fn discriminants_match() {
        assert_eq!(u8::from(NonceInstruction::Initialize), 0);
        assert_eq!(u8::from(NonceInstruction::Submit), 1);
        assert_eq!(u8::from(NonceInstruction::SetAuthority), 2);
        assert_eq!(u8::from(NonceInstruction::Close), 3);
    }

    #[test]
    fn try_from_rejects_unknown() {
        assert!(NonceInstruction::try_from(4).is_err());
        assert!(NonceInstruction::try_from(255).is_err());
    }
}
