use {
    crate::instruction::FALCON512_PUBLIC_KEY_LEN,
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
pub const INIT_NONCE_DERIVATION_TAG: &[u8] = b"spl-ed25519-durable-signer::init-v1";

/// Domain-separation tag for deriving a 32-byte Falcon authority id from a
/// Falcon public key.
pub const FALCON_AUTHORITY_DERIVATION_TAG: &[u8] =
    b"spl-ed25519-durable-signer::falcon512-authority-v1";

/// Serialized prepared Falcon-512 public key length used by `solana-falcon512`.
pub const FALCON512_PREPARED_PUBLIC_KEY_LEN: usize = 1024;

/// Authority material stored by the Falcon program variant.
///
/// Falcon public keys are too large to be Solana addresses or PDA seeds. The
/// `id` field is a 32-byte hash of the wire public key and is used anywhere the
/// generic durable signer logic needs an authority address or PDA seed. The
/// prepared public key is stored once at initialization so every submit can use
/// the lower-compute Falcon verification path without re-preparing the key.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct FalconAuthority {
    /// `sha256(FALCON_AUTHORITY_DERIVATION_TAG || falcon_public_key)`.
    pub id: Address,
    /// `solana_falcon512`'s serialized prepared public key.
    pub prepared_public_key: [u8; FALCON512_PREPARED_PUBLIC_KEY_LEN],
}

impl FalconAuthority {
    /// Serialized size of the Falcon authority material.
    pub const LEN: usize = ADDRESS_BYTES + FALCON512_PREPARED_PUBLIC_KEY_LEN;
}

/// Derives the 32-byte authority id used as a Falcon durable signer PDA seed.
pub fn falcon_authority_id(public_key: &[u8; FALCON512_PUBLIC_KEY_LEN]) -> Address {
    let hash = solana_sha256_hasher::hashv(&[FALCON_AUTHORITY_DERIVATION_TAG, public_key]);
    Address::new_from_array(*hash.as_bytes())
}

/// On-chain data for a caller-created durable signer account.
///
/// One authority can control any number of independent durable signer accounts. This is useful for
/// when that authority wants to prepare or submit more than one transaction concurrently. Each
/// account carries its own nonce, so consuming one nonce does not advance or invalidate
/// transactions prepared against another nonce state account.
#[derive(Clone, Debug, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct DurableSignerAccountData<Authority> {
    /// Single-use value that prevents a signed message from being replayed. `Submit` requires this
    /// to match the wrapped message's lifetime field: `lifetime_specifier` for `v1` messages or
    /// `recent_blockhash` for `legacy` and `v0` messages. On success, `Submit` advances it to a
    /// fresh hash over the prior nonce, `SlotHashes[0]`, and the wrapped message bytes.
    #[wincode(with = "HashBytes")]
    pub nonce: Hash,
    /// Scheme-specific authority material. Ed25519 stores the 32-byte authority
    /// address directly; Falcon stores a hash-derived authority id and a
    /// prepared public key.
    pub authority: Authority,
}

/// Standard Ed25519 durable signer state.
pub type DurableSignerAccount = DurableSignerAccountData<Address>;

/// Falcon-512 durable signer state.
pub type FalconDurableSignerAccount = DurableSignerAccountData<FalconAuthority>;

impl DurableSignerAccountData<Address> {
    /// Serialized account size: a 32-byte nonce followed by a 32-byte authority address.
    pub const LEN: usize = HASH_BYTES + ADDRESS_BYTES;
}

impl DurableSignerAccountData<FalconAuthority> {
    /// Serialized account size for the Falcon program variant.
    pub const LEN: usize = HASH_BYTES + FalconAuthority::LEN;
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
    use super::{
        Address, DurableSignerAccount, FalconAuthority, FalconDurableSignerAccount, Hash,
        falcon_authority_id,
    };

    #[test]
    fn len_matches_wincode_serialized_size() {
        let account = DurableSignerAccount {
            nonce: Hash::new_from_array([1; 32]),
            authority: Address::new_from_array([2; 32]),
        };

        assert_eq!(
            wincode::serialized_size(&account).unwrap() as usize,
            DurableSignerAccount::LEN
        );
    }

    #[test]
    fn falcon_len_matches_wincode_serialized_size() {
        let account = FalconDurableSignerAccount {
            nonce: Hash::new_from_array([1; 32]),
            authority: FalconAuthority {
                id: Address::new_from_array([2; 32]),
                prepared_public_key: [3; super::FALCON512_PREPARED_PUBLIC_KEY_LEN],
            },
        };

        assert_eq!(
            wincode::serialized_size(&account).unwrap() as usize,
            FalconDurableSignerAccount::LEN
        );
    }

    #[test]
    fn falcon_authority_id_is_domain_separated_and_stable() {
        let public_key = [7; super::FALCON512_PUBLIC_KEY_LEN];

        assert_eq!(
            falcon_authority_id(&public_key),
            falcon_authority_id(&public_key)
        );
        assert_ne!(
            falcon_authority_id(&public_key),
            Address::new_from_array(*solana_sha256_hasher::hash(&public_key).as_bytes())
        );
    }
}
