//! Verifies authority signatures over the submitted payload, then invokes the executor
//! it specifies, signing for each authority's programmatic signer.

use {
    alloc::vec::Vec,
    brine_ed25519::hasher::Sha512,
    pinocchio::{
        AccountView, Address, ProgramResult,
        cpi::{Seed, Signer, invoke_signed_with_slice},
        error::ProgramError,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_signature::Signature,
    spl_ed25519_signer_interface::{
        error::Error,
        instruction::{SubmitEnvelope, SubmitPayload},
        pda::ProgrammaticSigner,
    },
};

/// An authority whose signature verified, paired with the `ProgrammaticSigner` PDA to
/// promote on its behalf during the executor CPI.
struct AuthorizedSigner {
    authority: Address,
    programmatic_signer: Address,
    bump_seed: [u8; 1],
}

/// Verifies the envelope's signatures and replays the executor instruction it specifies by CPI,
/// promoting each authorized authority's `ProgrammaticSigner`.
pub fn process_submit(
    program_id: &Address,
    accounts: &[AccountView],
    envelope: SubmitEnvelope,
) -> ProgramResult {
    let SubmitEnvelope {
        signatures,
        payload,
    } = envelope;

    // At least one authority must sign
    if signatures.is_empty() {
        return Err(Error::NoSignatures.into());
    }

    // The signed payload specifies which signer program it authorizes, so an envelope
    // signed for a different signer program cannot be replayed against this one.
    if &payload.signer_program_id != program_id {
        return Err(Error::SignerProgramMismatch.into());
    }

    // One authority account per signature, then the executor program, then
    // the accounts forwarded to it.
    let Some((authority_accounts, [executor_program, forwarded_accounts @ ..])) =
        accounts.split_at_checked(signatures.len())
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // The caller must supply the executor account the payload specifies
    if executor_program.address() != &payload.executor_program_id {
        return Err(Error::ExecutorMismatch.into());
    }

    // Validate authorities signed the exact wincode-serialized payload
    let payload_bytes = payload
        .signing_bytes()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let authorized =
        authorize_signers(program_id, authority_accounts, &signatures, &payload_bytes)?;

    invoke_executor(&payload, forwarded_accounts, &authorized)
}

/// Verifies each envelope signature against the authority account at the same index and
/// derives the programmatic signer (PDA) it authorizes.
fn authorize_signers(
    program_id: &Address,
    authority_accounts: &[AccountView],
    signatures: &[Signature],
    payload_bytes: &[u8],
) -> Result<Vec<AuthorizedSigner>, ProgramError> {
    let mut authorized = Vec::with_capacity(signatures.len());
    for (authority_account, signature) in authority_accounts.iter().zip(signatures) {
        // Authorities pair with signatures by position index
        let authority = authority_account.address();
        brine_ed25519::verify::<Sha512>(authority, signature.as_array(), &[payload_bytes])
            .map_err(|_| ProgramError::from(Error::InvalidSignature))?;

        let (programmatic_signer, bump) =
            ProgrammaticSigner::derive_address_and_bump(program_id, authority);
        authorized.push(AuthorizedSigner {
            authority: Address::from(authority),
            programmatic_signer,
            bump_seed: [bump],
        });
    }

    Ok(authorized)
}

/// Invokes the executor with the forwarded accounts, preserving their privileges and
/// additionally signing for each authorized programmatic signer.
fn invoke_executor(
    payload: &SubmitPayload,
    forwarded_accounts: &[AccountView],
    authorized_signers: &[AuthorizedSigner],
) -> ProgramResult {
    // The PDA seeds that authorize `invoke_signed` to sign as each programmatic signer.
    let signer_seed_sets: Vec<[Seed; 3]> = authorized_signers
        .iter()
        .map(|signer| {
            [
                Seed::from(ProgrammaticSigner::SEED_PREFIX),
                Seed::from(signer.authority.as_ref()),
                Seed::from(&signer.bump_seed),
            ]
        })
        .collect();
    let cpi_signers: Vec<Signer> = signer_seed_sets.iter().map(Signer::from).collect();

    let mut instruction_accounts = Vec::with_capacity(forwarded_accounts.len());
    let mut account_views = Vec::with_capacity(forwarded_accounts.len());
    for account in forwarded_accounts {
        let is_promoted = authorized_signers
            .iter()
            .any(|signer| account.address() == &signer.programmatic_signer);

        instruction_accounts.push(InstructionAccount::new(
            account.address(),
            account.is_writable(),
            // If a real keypair signed the outer transaction, we propagate that signer
            // status down. If the account is the promoted PDA, we pass signer status.
            account.is_signer() || is_promoted,
        ));
        account_views.push(account);
    }

    let view = InstructionView {
        program_id: &payload.executor_program_id,
        accounts: &instruction_accounts,
        data: &payload.executor_instruction_data,
    };

    invoke_signed_with_slice::<&AccountView>(&view, &account_views, &cpi_signers)
}
