/// Instructions supported by the SPL Nonce program.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonceInstruction {
    /// Creates a new nonce state account for the given authority policy. Anyone
    /// may initialize the canonical pre-funded PDA for a given authority policy.
    ///
    /// On success, the state account is initialized with:
    /// - `nonce = 0`
    /// - the authority policy that will govern future signed actions
    ///
    /// The nonce state address is derived from [`NonceStatePda`](crate::pda::NonceStatePda)
    /// and must already be pre-funded with enough lamports to be rent-exempt before
    /// this instruction runs. During initialization, the program claims that PDA
    /// as nonce state and writes the initial state into it.
    ///
    /// Accounts required:
    /// - `[writable]` Nonce state PDA to initialize (pre-funded)
    /// - `[]` System program used to allocate and assign the pre-funded PDA
    Initialize,

    /// Verifies threshold Ed25519 signatures over a signed message, then
    /// performs the action committed in that message.
    ///
    /// The signed message specifies one of:
    /// - `Execute`: run signed CPI instructions
    /// - `AdvanceNonce`: increment the nonce, invalidating all previously signed messages
    /// - `Close`: close the state account and refund lamports
    ///
    /// On success, the program:
    /// 1. Verifies that enough authority-policy members signed the message
    /// 2. Checks the message nonce matches the state and the deadline has not passed
    /// 3. Performs the committed action
    ///
    /// The instruction data format is defined in [`message`](crate::message).
    ///
    /// Accounts required:
    /// - `[writable]` Nonce state PDA
    /// - Remaining: accounts from the signed message's account table, in the
    ///   exact same order. `AdvanceNonce` requires no remaining accounts.
    Submit,
}

impl TryFrom<u8> for NonceInstruction {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Initialize),
            1 => Ok(Self::Submit),
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
    }

    #[test]
    fn try_from_rejects_unknown() {
        assert!(NonceInstruction::try_from(2).is_err());
        assert!(NonceInstruction::try_from(255).is_err());
    }
}
