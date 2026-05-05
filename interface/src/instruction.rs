use {
    solana_address::Address,
    wincode::{SchemaRead, SchemaWrite},
};

/// Instructions supported by the SPL Ed25519 Durable Signer program.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurableSignerInstruction {
    /// Initializes a durable signer account for an authority.
    ///
    /// The caller must first create and fund the account. Recommended to include
    /// `solana_system_interface::instruction::create_account` and `Initialize` in the same
    /// transaction so no other transaction can initialize the account first.
    ///
    /// On success, the program:
    /// 1. Verifies the account is uninitialized, rent-exempt, and owned by this program.
    /// 2. Derives the initial `nonce` as
    ///    `sha256("spl-ed25519-durable-signer::init-v1" ‖ durable_signer_account_address ‖ slot_hashes[0])`.
    /// 3. Writes `DurableSignerAccount { nonce, authority }` into the account data.
    ///
    /// Instruction data: empty.
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account
    /// - `[]` Authority to store in the durable signer account
    /// - `[]` `SlotHashes` sysvar
    Initialize,

    /// Authorizes and executes a wrapped Solana transaction whose required signers are
    /// `DurableSignerPda` accounts.
    ///
    /// Instruction data: serialized `solana_transaction::versioned::VersionedTransaction`.
    /// All message variants supported by `VersionedTransaction` are accepted.
    ///
    /// Wrapped required signers are paired by index:
    /// - `message.account_keys[i]`: `DurableSignerPda` promoted during CPI.
    /// - `tx.signatures[i]`: wrapped-message signature from the matching authority address.
    ///
    /// On success, the program:
    /// 1. Deserializes the transaction and sanitizes the wrapped message.
    /// 2. Reads the authority stored in the durable signer account.
    /// 3. Checks the passed durable signer account's authority signed the wrapped message.
    /// 4. Checks the wrapped message's lifetime / recent blockhash field equals the account's
    ///    `nonce`.
    /// 5. Verifies the outer transaction's only top-level instruction is `Submit`.
    /// 6. For each wrapped required signer position `i`, requires
    ///    `DurableSignerPda(authority_i) == message.account_keys[i]` and verifies
    ///    `tx.signatures[i]` over the wrapped message with `authority_i`.
    /// 7. Executes each `message.instructions` entry by CPI, using `invoke_signed` to promote
    ///    each authorized signer's corresponding `DurableSignerPda`.
    /// 8. Derives and stores the next nonce as
    ///    `sha256("spl-ed25519-durable-signer::v1" ‖ durable_signer_account ‖ old_nonce ‖ slot_hashes[0] ‖ sha256(signed_message_bytes))`
    ///
    /// Accounts required:
    /// - `[writable]` Durable signer account whose nonce is consumed and advanced
    /// - `[]` `SlotHashes` sysvar
    /// - `[]` `Instructions` sysvar
    /// - Required-signer authority addresses, ordered to match the wrapped required signers:
    ///   `DurableSignerPda(authority_i) == message.account_keys[i]`.
    /// - Remaining: all accounts referenced by the wrapped message, in order, with `is_signer`
    ///   and `is_writable` flags matching the wrapped message.
    Submit,

    /// Closes a durable signer account and refunds its lamports.
    ///
    /// Instruction data: [`CloseData`].
    ///
    /// Runs only as an inner instruction of a wrapped transaction submitted through `Submit`
    /// because nothing outside this program can sign for `DurableSignerPda`.
    ///
    /// Accounts required:
    /// - `[signer]` `DurableSignerPda`
    /// - `[writable]` Durable signer account
    /// - `[writable]` Lamport recipient
    Close,
}

/// Data for [`DurableSignerInstruction::Close`].
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct CloseData {
    /// Address that receives all lamports from the closed durable signer account.
    pub recipient: Address,
}

impl TryFrom<u8> for DurableSignerInstruction {
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

impl From<DurableSignerInstruction> for u8 {
    fn from(value: DurableSignerInstruction) -> Self {
        value as u8
    }
}

#[cfg(test)]
mod tests {
    use super::DurableSignerInstruction;

    #[test]
    fn discriminants_match() {
        assert_eq!(u8::from(DurableSignerInstruction::Initialize), 0);
        assert_eq!(u8::from(DurableSignerInstruction::Submit), 1);
        assert_eq!(u8::from(DurableSignerInstruction::Close), 2);
    }

    #[test]
    fn try_from_rejects_unknown() {
        assert!(DurableSignerInstruction::try_from(4).is_err());
        assert!(DurableSignerInstruction::try_from(255).is_err());
    }
}
