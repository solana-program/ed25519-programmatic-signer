use {
    crate::{initialize::process_initialize, submit::process_submit},
    pinocchio::{AccountView, Address, ProgramResult, error::ProgramError},
    spl_ed25519_durable_signer_interface::instruction::DurableSignerInstruction,
};

#[inline(always)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match DurableSignerInstruction::try_from_bytes(instruction_data)? {
        DurableSignerInstruction::Initialize => process_initialize(program_id, accounts),
        DurableSignerInstruction::Submit(transaction) => {
            process_submit(program_id, accounts, instruction_data, transaction)
        }
        DurableSignerInstruction::Close => Err(ProgramError::InvalidInstructionData),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        alloc::vec,
        solana_transaction::{Message, VersionedMessage, versioned::VersionedTransaction},
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
            DurableSignerInstruction::Initialize
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
            process_instruction(&Address::default(), &mut accounts, &[2]).unwrap_err(),
            ProgramError::InvalidInstructionData
        );
    }
}
