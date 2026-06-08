//! Falcon-512 signing scheme for the durable signer program.
//!
//! Falcon cannot use the native Solana
//! [`VersionedTransaction`](solana_transaction::versioned::VersionedTransaction)
//! signature vector:
//! Falcon public keys are 897 bytes and Falcon signatures are 666 bytes, while
//! native transaction signatures are fixed at 64 bytes. The Falcon program
//! variant therefore uses a scheme-specific submit envelope:
//! [`FalconSubmit`].
//!
//! ## Authority binding
//!
//! The durable signer PDA still needs a 32-byte seed. Falcon derives that seed
//! from the public key:
//!
//! ```text
//! authority_id = sha256(FALCON_AUTHORITY_DERIVATION_TAG || falcon_public_key)
//! durable_pda  = PDA("durable-signer", authority_id)
//! ```
//!
//! The public key is supplied once during `Initialize`, hashed into the 32-byte
//! authority id, and converted to `solana-falcon512`'s prepared form for
//! lower-compute verification. Each `Submit` then carries only the Falcon
//! signature for the wrapped message.

use {
    crate::verifier::{ParsedInitializeAccounts, SigningScheme, VerifiedSigner},
    pinocchio::{AccountView, Address, error::ProgramError},
    solana_falcon512::{
        FALCON_512_PREPARED_PUBKEY_LEN, FALCON_512_PUBKEY_LEN, FALCON_512_SIGNATURE_LEN,
        Falcon512PreparedPubkey, Falcon512Pubkey, Falcon512Signature,
    },
    solana_transaction::VersionedMessage,
    spl_ed25519_durable_signer_interface::{
        error::DurableSignerError,
        instruction::{
            FALCON512_PUBLIC_KEY_LEN, FALCON512_SIGNATURE_LEN, FalconInitialize, FalconSubmit,
        },
        pda::DurableSignerPda,
        state::{
            FALCON512_PREPARED_PUBLIC_KEY_LEN, FalconAuthority, FalconDurableSignerAccount,
            falcon_authority_id,
        },
    },
};

const _: () = assert!(
    FALCON_512_PUBKEY_LEN == FALCON512_PUBLIC_KEY_LEN,
    "interface and verifier public-key lengths must match",
);
const _: () = assert!(
    FALCON_512_SIGNATURE_LEN == FALCON512_SIGNATURE_LEN,
    "interface and verifier signature lengths must match",
);
const _: () = assert!(
    FALCON_512_PREPARED_PUBKEY_LEN == FALCON512_PREPARED_PUBLIC_KEY_LEN,
    "interface and verifier prepared-key lengths must match",
);

/// Falcon-512 signing scheme.
pub struct FalconScheme;

impl SigningScheme for FalconScheme {
    type Initialize = FalconInitialize;
    type Submit = FalconSubmit;
    type Authority = FalconAuthority;

    const STATE_LEN: usize = FalconDurableSignerAccount::LEN;

    fn parse_initialize_accounts<'a>(
        accounts: &'a [AccountView],
        initialize: &Self::Initialize,
    ) -> Result<ParsedInitializeAccounts<'a, Self::Authority>, ProgramError> {
        let [slot_hashes_account, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        let public_key = Falcon512Pubkey::try_from_slice(&initialize.public_key)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        let prepared = public_key
            .try_prepare_pubkey()
            .map_err(|_| ProgramError::InvalidInstructionData)?;

        Ok(ParsedInitializeAccounts {
            authority: FalconAuthority {
                id: falcon_authority_id(&initialize.public_key),
                prepared_public_key: *prepared.as_bytes(),
            },
            slot_hashes_account,
        })
    }

    #[inline(always)]
    fn message(submit: &Self::Submit) -> &VersionedMessage {
        &submit.message
    }

    fn validate_submit(submit: &Self::Submit, signer_count: usize) -> Result<(), ProgramError> {
        // This first Falcon variant stores one prepared public key in the
        // durable signer account, so it can authorize exactly one required PDA
        // signer. Multi-signer Falcon support should use authority/key accounts
        // or a registry so each signer has distinct key material.
        if signer_count != 1 || submit.signatures.len() != 1 {
            return Err(DurableSignerError::InvalidWrappedTransaction.into());
        }
        Ok(())
    }

    fn authority_account_count(_signer_count: usize) -> Result<usize, ProgramError> {
        Ok(0)
    }

    fn verify_signer(
        program_id: &Address,
        state_authority: &Self::Authority,
        _authority_accounts: &[AccountView],
        signer_index: usize,
        expected_pda: &Address,
        submit: &Self::Submit,
        message_bytes: &[u8],
    ) -> Result<VerifiedSigner, ProgramError> {
        if signer_index != 0 {
            return Err(DurableSignerError::InvalidWrappedTransaction.into());
        }

        let (pda, bump) =
            DurableSignerPda::derive_address_and_bump(program_id, &state_authority.id);
        if &pda != expected_pda {
            return Err(DurableSignerError::IncorrectAuthorityPda.into());
        }

        let prepared =
            Falcon512PreparedPubkey::try_from_slice(&state_authority.prepared_public_key)
                .map_err(|_| ProgramError::InvalidAccountData)?;
        let signature = submit
            .signatures
            .get(signer_index)
            .ok_or(ProgramError::from(
                DurableSignerError::InvalidWrappedTransaction,
            ))?;
        let signature = Falcon512Signature::try_from_slice(signature.as_ref())
            .map_err(|_| ProgramError::from(DurableSignerError::MissingAuthorization))?;

        if !signature.verify_with_prepared(message_bytes, prepared) {
            return Err(DurableSignerError::MissingAuthorization.into());
        }

        Ok(VerifiedSigner {
            authority: state_authority.id,
            bump,
            is_state_authority: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        pqcrypto_falcon::falcon512,
        pqcrypto_traits::sign::{DetachedSignature, PublicKey},
        solana_falcon512::Falcon512Pubkey,
        spl_ed25519_durable_signer_interface::instruction::FalconSignature as InterfaceSignature,
    };

    fn padded_signature(raw: &[u8]) -> InterfaceSignature {
        InterfaceSignature::try_from_compressed(raw).unwrap()
    }

    #[test]
    fn pqclean_signature_verifies_with_prepared_key() {
        let (pk, sk) = falcon512::keypair();
        let message = b"durable signer falcon round-trip";
        let detached = falcon512::detached_sign(message, &sk);
        let signature = padded_signature(detached.as_bytes());

        let public_key = Falcon512Pubkey::try_from_slice(pk.as_bytes()).unwrap();
        let prepared = public_key.try_prepare_pubkey().unwrap();
        let signature = Falcon512Signature::try_from_slice(signature.as_ref()).unwrap();

        assert!(signature.verify_with_prepared(message, &prepared));
        assert!(!signature.verify_with_prepared(b"a different message", &prepared));
    }
}
