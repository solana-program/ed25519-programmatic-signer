//! Signing-scheme abstraction for the durable signer processor.
//!
//! The shared processor only needs a wrapped [`VersionedMessage`], a stable
//! 32-byte authority id for PDA derivation, and a way to verify each required
//! signer. Everything else is scheme-specific:
//!
//! - Ed25519 keeps the native Solana [`VersionedTransaction`] envelope so normal
//!   wallet and command-line flows can sign the wrapped message.
//! - Falcon uses a custom submit envelope because Falcon signatures are much
//!   larger than the fixed 64-byte Solana transaction signature slots.

#[cfg(any(not(feature = "falcon"), test))]
use brine_ed25519::{hasher::Sha512, verify};
use {
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_transaction::VersionedMessage,
    spl_ed25519_durable_signer_interface::{
        instruction::DurableSignerInstructionData, state::DurableSignerAccountData,
    },
};
#[cfg(any(not(feature = "falcon"), test))]
use {
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_durable_signer_interface::{
        error::DurableSignerError, state::DurableSignerAccount,
    },
};

/// Result of verifying one wrapped required signer.
pub struct VerifiedSigner {
    /// Authority id used as the durable signer PDA seed.
    pub authority: Address,
    /// Bump for `PDA("durable-signer", authority)`.
    pub bump: u8,
    /// Whether this signer is the authority stored in the durable signer state.
    pub is_state_authority: bool,
}

/// Scheme-specific authority material plus the Slot Hashes account used during
/// initialization.
pub struct ParsedInitializeAccounts<'a, Authority> {
    /// Authority material to store in the durable signer account.
    pub authority: Authority,
    /// Slot Hashes sysvar account used to derive the first durable nonce.
    pub slot_hashes_account: &'a AccountView,
}

/// Compile-time signing scheme selected by `crate::config`.
///
/// A deployed program has exactly one active scheme. This keeps the wire format
/// and state layout simple while still proving that the nonce/PDA/CPI machinery
/// is generic over the authorization mechanism.
pub trait SigningScheme {
    /// Scheme-specific payload for `Initialize`.
    type Initialize;
    /// Scheme-specific signed envelope for `Submit`.
    type Submit;
    /// Scheme-specific authority material stored in the durable signer account.
    type Authority;

    /// Serialized durable signer account length for this scheme.
    const STATE_LEN: usize;

    /// Parses the scheme-specific initialize accounts.
    ///
    /// Ed25519 receives an authority address account followed by Slot Hashes.
    /// Falcon stores the authority key material in instruction data, so it only
    /// receives Slot Hashes.
    fn parse_initialize_accounts<'a>(
        accounts: &'a [AccountView],
        initialize: &Self::Initialize,
    ) -> Result<ParsedInitializeAccounts<'a, Self::Authority>, ProgramError>;

    /// Returns the wrapped message carried by this scheme's submit envelope.
    fn message(submit: &Self::Submit) -> &VersionedMessage;

    /// Validates scheme-specific submit envelope fields against the wrapped
    /// message signer count.
    fn validate_submit(submit: &Self::Submit, signer_count: usize) -> Result<(), ProgramError>;

    /// Number of authority accounts this scheme expects before wrapped message
    /// accounts in `Submit`'s remaining account list.
    fn authority_account_count(signer_count: usize) -> Result<usize, ProgramError>;

    /// Verifies signer `signer_index` and proves it corresponds to
    /// `expected_pda`.
    fn verify_signer(
        program_id: &Address,
        state_authority: &Self::Authority,
        authority_accounts: &[AccountView],
        signer_index: usize,
        expected_pda: &Address,
        submit: &Self::Submit,
        message_bytes: &[u8],
    ) -> Result<VerifiedSigner, ProgramError>;
}

/// Concrete instruction envelope for a signing scheme.
pub type SchemeInstruction<S> =
    DurableSignerInstructionData<<S as SigningScheme>::Initialize, <S as SigningScheme>::Submit>;

/// Concrete durable signer account state for a signing scheme.
pub type SchemeState<S> = DurableSignerAccountData<<S as SigningScheme>::Authority>;

/// Standard Solana Ed25519 signing scheme.
#[cfg(any(not(feature = "falcon"), test))]
pub struct Ed25519Scheme;

#[cfg(any(not(feature = "falcon"), test))]
impl SigningScheme for Ed25519Scheme {
    type Initialize = ();
    type Submit = VersionedTransaction;
    type Authority = Address;

    const STATE_LEN: usize = DurableSignerAccount::LEN;

    fn parse_initialize_accounts<'a>(
        accounts: &'a [AccountView],
        _initialize: &Self::Initialize,
    ) -> Result<ParsedInitializeAccounts<'a, Self::Authority>, ProgramError> {
        let [authority, slot_hashes_account, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        Ok(ParsedInitializeAccounts {
            authority: Address::from(authority.address()),
            slot_hashes_account,
        })
    }

    #[inline(always)]
    fn message(submit: &Self::Submit) -> &VersionedMessage {
        &submit.message
    }

    fn validate_submit(submit: &Self::Submit, signer_count: usize) -> Result<(), ProgramError> {
        if submit.signatures.len() != signer_count {
            return Err(DurableSignerError::InvalidWrappedTransaction.into());
        }
        Ok(())
    }

    #[inline(always)]
    fn authority_account_count(signer_count: usize) -> Result<usize, ProgramError> {
        Ok(signer_count)
    }

    fn verify_signer(
        program_id: &Address,
        state_authority: &Self::Authority,
        authority_accounts: &[AccountView],
        signer_index: usize,
        expected_pda: &Address,
        submit: &Self::Submit,
        message_bytes: &[u8],
    ) -> Result<VerifiedSigner, ProgramError> {
        let authority_account = authority_accounts
            .get(signer_index)
            .ok_or(ProgramError::from(
                DurableSignerError::InvalidWrappedTransaction,
            ))?;
        let authority = Address::from(authority_account.address());
        let (pda, bump) =
            spl_ed25519_durable_signer_interface::pda::DurableSignerPda::derive_address_and_bump(
                program_id, &authority,
            );
        if &pda != expected_pda {
            return Err(DurableSignerError::IncorrectAuthorityPda.into());
        }

        let pubkey: &[u8; 32] = authority
            .as_ref()
            .try_into()
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;
        let signature = submit
            .signatures
            .get(signer_index)
            .ok_or(ProgramError::from(
                DurableSignerError::InvalidWrappedTransaction,
            ))?;
        let signature: &[u8; 64] = signature
            .as_ref()
            .try_into()
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;

        verify::<Sha512>(pubkey, signature, &[message_bytes])
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;

        Ok(VerifiedSigner {
            authority,
            bump,
            is_state_authority: &authority == state_authority,
        })
    }
}
