use {
    crate::{
        initialize::process_initialize,
        submit::process_submit,
        verifier::{SchemeInstruction, SchemeState, SigningScheme},
    },
    pinocchio::{AccountView, Address, ProgramResult, error::ProgramError},
    spl_ed25519_durable_signer_interface::instruction::DurableSignerInstructionData,
    wincode::{SchemaRead, SchemaWrite, config::DefaultConfig},
};

#[inline(never)]
pub fn process_instruction<S: SigningScheme>(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult
where
    SchemeInstruction<S>: for<'de> SchemaRead<'de, DefaultConfig, Dst = SchemeInstruction<S>>,
    SchemeState<S>: for<'de> SchemaRead<'de, DefaultConfig, Dst = SchemeState<S>>
        + SchemaWrite<DefaultConfig, Src = SchemeState<S>>,
{
    match SchemeInstruction::<S>::try_from_bytes(instruction_data)? {
        DurableSignerInstructionData::Initialize(initialize) => {
            process_initialize::<S>(program_id, accounts, &initialize)
        }
        DurableSignerInstructionData::Submit(submit) => {
            process_submit::<S>(program_id, accounts, instruction_data, submit)
        }
        DurableSignerInstructionData::Close => Err(ProgramError::InvalidInstructionData),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::verifier::Ed25519Scheme,
        alloc::vec,
        solana_transaction::{Message, VersionedMessage, versioned::VersionedTransaction},
        spl_ed25519_durable_signer_interface::instruction::DurableSignerInstruction,
    };

    fn empty_transaction() -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message::default()),
        }
    }

    #[test]
    fn parse_instruction_valid_discriminators() {
        assert!(matches!(
            DurableSignerInstruction::try_from_bytes(&[0]).unwrap(),
            DurableSignerInstruction::Initialize(())
        ));
        assert!(matches!(
            DurableSignerInstruction::try_from_bytes(
                &wincode::serialize(&DurableSignerInstruction::Submit(empty_transaction()))
                    .unwrap(),
            )
            .unwrap(),
            DurableSignerInstruction::Submit(_)
        ));
    }

    #[test]
    fn parse_instruction_rejects_invalid() {
        assert!(DurableSignerInstruction::try_from_bytes(&[]).is_err());
        assert!(DurableSignerInstruction::try_from_bytes(&[3]).is_err());
        assert!(DurableSignerInstruction::try_from_bytes(&[255]).is_err());
        assert!(DurableSignerInstruction::try_from_bytes(&[0, 0]).is_err());
    }

    #[test]
    fn close_returns_deterministic_error_until_implemented() {
        let mut accounts = [];

        assert_eq!(
            process_instruction::<Ed25519Scheme>(&Address::default(), &mut accounts, &[2])
                .unwrap_err(),
            ProgramError::InvalidInstructionData
        );
    }
}
