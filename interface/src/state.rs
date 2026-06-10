use {
    core::mem::MaybeUninit,
    solana_address::{ADDRESS_BYTES, Address},
    solana_hash::{HASH_BYTES, Hash},
    wincode::{
        ReadResult, SchemaRead, SchemaWrite, TypeMeta, WriteResult,
        config::Config,
        io::{Reader, Writer},
    },
};

/// Tag for the initial nonce derivation.
pub const INIT_NONCE_DERIVATION_TAG: &[u8] = b"spl-ed25519-programmatic-signer::init-v1";

/// On-chain data for a caller-created programmatic signer account.
///
/// One authority can control any number of independent programmatic signer accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently. Each
/// account carries its own nonce, so consuming one nonce does not advance or invalidate
/// transactions prepared against another programmatic signer account.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct ProgrammaticSignerAccount {
    /// Single-use value that prevents a signed message from being replayed. `Submit` requires this
    /// to match the wrapped message's lifetime field: `lifetime_specifier` for `v1` messages or
    /// `recent_blockhash` for `legacy` and `v0` messages. On success, `Submit` advances it to a
    /// fresh hash over the prior nonce, `SlotHashes[0]`, and the wrapped message bytes.
    #[wincode(with = "HashBytes")]
    pub nonce: Hash,
    /// Address allowed to consume this nonce and advance its value. `Submit` verifies that this
    /// address signed the wrapped transaction message.
    pub authority: Address,
}

impl ProgrammaticSignerAccount {
    /// Serialized account size: a 32-byte nonce followed by a 32-byte authority address.
    pub const LEN: usize = HASH_BYTES + ADDRESS_BYTES;
}

// TODO: Remove `HashBytes` and enable `solana-hash` wincode feature once the Mollusk/Agave
//       dependency graph allows a `solana-hash` release whose wincode feature is 0.5.x.
struct HashBytes;

unsafe impl<C: Config> SchemaWrite<C> for HashBytes {
    type Src = Hash;

    const TYPE_META: TypeMeta = <[u8; HASH_BYTES] as SchemaWrite<C>>::TYPE_META;

    #[inline(always)]
    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        <[u8; HASH_BYTES] as SchemaWrite<C>>::size_of(src.as_bytes())
    }

    #[inline(always)]
    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        <[u8; HASH_BYTES] as SchemaWrite<C>>::write(writer, src.as_bytes())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for HashBytes {
    type Dst = Hash;

    const TYPE_META: TypeMeta = <[u8; HASH_BYTES] as SchemaRead<'de, C>>::TYPE_META;

    #[inline(always)]
    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let bytes = <[u8; HASH_BYTES] as SchemaRead<'de, C>>::get(reader)?;
        dst.write(Hash::new_from_array(bytes));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, Hash, ProgrammaticSignerAccount};

    #[test]
    fn len_matches_wincode_serialized_size() {
        let account = ProgrammaticSignerAccount {
            nonce: Hash::new_from_array([1; 32]),
            authority: Address::new_from_array([2; 32]),
        };

        assert_eq!(
            wincode::serialized_size(&account).unwrap() as usize,
            ProgrammaticSignerAccount::LEN
        );
    }
}
