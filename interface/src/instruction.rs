use {
    solana_address::Address,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Nonce program.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonceInstruction {
    /// Initializes a nonce state account for an authority.
    ///
    /// The caller must first create and fund the nonce state account. Recommended to include
    /// `solana_system_interface::instruction::create_account` and `Initialize` in the same
    /// transaction so no other transaction can initialize the account first.
    ///
    /// On success, the program:
    /// 1. Verifies the nonce state account is uninitialized, rent-exempt, and owned by
    ///    the nonce program.
    /// 2. Derives the initial `nonce` as
    ///    `sha256("spl-nonce::init-v1" ‖ nonce_state_address ‖ slot_hashes[0])`.
    /// 3. Writes `NonceState { nonce, authority }` into the account data.
    ///
    /// Instruction data: empty.
    ///
    /// Accounts required:
    /// - `[writable]` Nonce state account
    /// - `[]` Authority to store in the nonce state account
    /// - `[]` `SlotHashes` sysvar
    Initialize,

    /// Authorizes and executes a wrapped Solana transaction whose required signers are
    /// `NonceAuthorityPda` accounts.
    ///
    /// Instruction data: serialized `solana_transaction::versioned::VersionedTransaction`.
    /// All message variants supported by `VersionedTransaction` are accepted.
    ///
    /// Wrapped required signers are paired by index:
    /// - `message.account_keys[i]`: `NonceAuthorityPda` promoted during CPI.
    /// - `tx.signatures[i]`: wrapped-message signature from the matching authority address.
    ///
    /// On success, the program:
    /// 1. Deserializes the transaction and sanitizes the wrapped message.
    /// 2. Reads the authority stored in the nonce state account.
    /// 3. Checks the passed nonce state account's authority signed the wrapped message.
    /// 4. Checks the wrapped message's lifetime / recent blockhash field equals `state.nonce`.
    /// 5. Verifies the outer transaction's only top-level instruction is `Submit`.
    /// 6. For each wrapped required signer position `i`, requires
    ///    `NonceAuthorityPda(authority_i) == message.account_keys[i]` and verifies
    ///    `tx.signatures[i]` over the wrapped message with `authority_i`.
    /// 7. Executes each `message.instructions` entry by CPI, using `invoke_signed` to promote
    ///    each authorized signer's corresponding `NonceAuthorityPda`.
    /// 8. Derives and stores the next nonce as
    ///    `sha256("spl-nonce::v1" ‖ nonce_state ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(signed_message_bytes))`
    ///
    /// Accounts required:
    /// - `[writable]` Nonce state account whose nonce is consumed and advanced
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Required-signer authority addresses, ordered to match the wrapped required signers:
    ///   `NonceAuthorityPda(authority_i) == message.account_keys[i]`.
    /// - Remaining: all accounts referenced by the wrapped message, in order, with `is_signer`
    ///   and `is_writable` flags matching the wrapped message.
    Submit,

    /// Closes a nonce state account and refunds its lamports.
    ///
    /// Instruction data: [`CloseData`].
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`
    /// because nothing outside this program can sign for `NonceAuthorityPda`.
    ///
    /// Accounts required:
    /// - `[signer]` `NonceAuthorityPda`
    /// - `[writable]` Nonce state account
    /// - `[writable]` Lamport recipient
    Close,
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
            2 => Ok(Self::Close),
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
        assert_eq!(u8::from(NonceInstruction::Close), 2);
    }

    #[test]
    fn try_from_rejects_unknown() {
        assert!(NonceInstruction::try_from(4).is_err());
        assert!(NonceInstruction::try_from(255).is_err());
    }
}
