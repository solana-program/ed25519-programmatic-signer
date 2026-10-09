//! v1 transaction messages for simulating `Execute` and relaying `Submit`.

use {
    anyhow::{Context, Result},
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_message::v1,
};

/// Runtime maximum compute unit limit.
const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// Runtime maximum loaded accounts data size, in bytes.
const MAX_LOADED_ACCOUNTS_DATA_SIZE: u32 = 64 * 1024 * 1024;

/// Unset v1 limits are zero rather than the legacy defaults, so a transaction requests the
/// maximum. Every run must build the same relay message, including `--sign-only` runs that make no
/// RPC calls, so the limits are fixed rather than sized from a simulation.
// TODO later: Align with how other CLIs set v1 limits.
const LIMITS: v1::TransactionConfig = v1::TransactionConfig::empty()
    .with_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT)
    .with_loaded_accounts_data_size_limit(MAX_LOADED_ACCOUNTS_DATA_SIZE);

/// Compile and validate a v1 message for `instructions`.
pub(super) fn v1_message(
    fee_payer: &Address,
    instructions: &[Instruction],
    blockhash: Hash,
) -> Result<v1::Message> {
    let message = v1::Message::try_compile_with_config(fee_payer, instructions, blockhash, LIMITS)
        .context("failed to compile transaction message")?;
    message.validate().context("invalid transaction message")?;
    Ok(message)
}
