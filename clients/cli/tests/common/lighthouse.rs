//! Lighthouse assertions for execution messages.
//!
//! The fixture `tests/fixtures/lighthouse.so` is the immutable mainnet deployment of
//! `L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95` (last deployed in slot 294179293), dumped with
//! `solana program dump` (sha256
//! `a89179ae024ac36aa5fc308251caa84cd0a3390ae66d8523514004a86d5148bb`).
//!
//! Source: <https://github.com/Jac0xb/lighthouse>.

use {solana_address::Address, solana_instruction::Instruction};

/// `LighthouseInstruction::AssertSysvarClock` discriminator.
const ASSERT_SYSVAR_CLOCK: u8 = 15;
/// `LogLevel::Silent`.
const LOG_LEVEL_SILENT: u8 = 0;
/// `SysvarClockAssertion::Slot`.
const CLOCK_SLOT: u8 = 0;
/// `IntegerOperator::LessThanOrEqual`.
const LESS_THAN_OR_EQUAL: u8 = 5;

/// `LighthouseError::AssertionFailed` (6001) as it appears in transaction errors.
pub const ASSERTION_FAILED: &str = "custom program error: 0x1771";

pub fn id() -> Address {
    "L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95"
        .parse()
        .unwrap()
}

/// An instruction that fails once the current slot is after `last_valid_slot`.
pub fn assert_slot_at_most(last_valid_slot: u64) -> Instruction {
    let mut data = vec![ASSERT_SYSVAR_CLOCK, LOG_LEVEL_SILENT, CLOCK_SLOT];
    data.extend_from_slice(&last_valid_slot.to_le_bytes());
    data.push(LESS_THAN_OR_EQUAL);
    Instruction::new_with_bytes(id(), &data, vec![])
}
