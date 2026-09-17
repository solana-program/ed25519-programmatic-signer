//! Builder for the on-chain `Submit` instruction.

use {
    alloc::{collections::BTreeSet, vec::Vec},
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::VersionedMessage,
    solana_signature::Signature,
    spl_ed25519_signer_interface::instruction::Instruction as SignerInstruction,
};

/// Builds the `Submit` instruction from signatures and their signed message.
pub fn submit(signatures: Vec<Signature>, message: VersionedMessage) -> Instruction {
    submit_with_outer_signers(signatures, message, &[])
}

/// Builds `Submit`, marking matching message accounts as transaction signers.
///
/// Use this instead of [`submit`] when an executed inner instruction needs a signature,
/// such as requiring a specific relayer to approve execution. The signer program only
/// forwards signer permission for accounts that sign both `message` and the transaction
/// containing `Submit`.
pub fn submit_with_outer_signers(
    signatures: Vec<Signature>,
    message: VersionedMessage,
    outer_signers: &[Address],
) -> Instruction {
    let accounts = message
        .static_account_keys()
        .iter()
        .enumerate()
        .map(|(index, key)| AccountMeta {
            pubkey: *key,
            // Requested outer transaction signatures
            is_signer: outer_signers.contains(key),
            is_writable: message
                .is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>),
        })
        .collect();
    Instruction::new_with_wincode(
        spl_ed25519_signer_interface::id(),
        &SignerInstruction::Submit {
            signatures,
            message,
        },
        accounts,
    )
}

#[cfg(test)]
mod tests {
    use {super::*, alloc::vec, solana_hash::Hash, solana_message::legacy::Message};

    #[test]
    fn outer_signers_only_change_matching_account_flags() {
        let authority = Address::new_from_array([1; 32]);
        let other = Address::new_from_array([2; 32]);
        let unsigned = Address::new_from_array([3; 32]);
        let message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
            2,
            0,
            1,
            vec![authority, other, unsigned],
            Hash::default(),
            vec![],
        ));
        let signatures = vec![Signature::default(); 2];
        let original = submit(signatures.clone(), message.clone());
        assert!(original.accounts.iter().all(|account| !account.is_signer));
        assert_eq!(
            submit_with_outer_signers(signatures.clone(), message.clone(), &[]),
            original
        );
        assert_eq!(
            submit_with_outer_signers(
                signatures.clone(),
                message.clone(),
                &[Address::new_from_array([4; 32])]
            ),
            original
        );
        let mut expected = original.accounts.clone();
        expected[2].is_signer = true;
        assert_eq!(
            submit_with_outer_signers(signatures.clone(), message.clone(), &[unsigned]).accounts,
            expected
        );
        let forwarded =
            submit_with_outer_signers(signatures.clone(), message.clone(), &[authority, authority]);
        assert_eq!(forwarded.program_id, original.program_id);
        assert_eq!(forwarded.data, original.data);
        let mut expected = original.accounts;
        expected[0].is_signer = true;
        assert_eq!(forwarded.accounts, expected);
        let forwarded = submit_with_outer_signers(signatures, message, &[authority, other]);
        expected[1].is_signer = true;
        assert_eq!(forwarded.accounts, expected);
    }
}
