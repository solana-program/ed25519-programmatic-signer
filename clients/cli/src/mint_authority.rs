//! The authorities of a Token or Token-2022 mint, including those of Token-2022 extensions.

use {
    anyhow::Result,
    clap::ValueEnum,
    solana_address::Address,
    spl_token_2022_interface::{
        extension::{
            BaseStateWithExtensions, ExtensionType, StateWithExtensions,
            confidential_transfer::ConfidentialTransferMint,
            confidential_transfer_fee::ConfidentialTransferFeeConfig,
            group_member_pointer::GroupMemberPointer, group_pointer::GroupPointer,
            interest_bearing_mint::InterestBearingConfig, metadata_pointer::MetadataPointer,
            mint_close_authority::MintCloseAuthority, pausable::PausableConfig,
            permanent_delegate::PermanentDelegate, permissioned_burn::PermissionedBurnConfig,
            scaled_ui_amount::ScaledUiAmountConfig, transfer_fee::TransferFeeConfig,
            transfer_hook::TransferHook,
        },
        instruction::AuthorityType as TokenAuthorityType,
        state::Mint,
    },
    spl_token_group_interface::state::TokenGroup,
    spl_token_metadata_interface::state::TokenMetadata,
    std::collections::BTreeMap,
};

/// An authority of a mint. All but `Mint` and `Freeze` belong to a Token-2022 extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum AuthorityType {
    Mint,
    Freeze,
    TransferFeeConfig,
    WithheldWithdraw,
    CloseMint,
    InterestRate,
    PermanentDelegate,
    ConfidentialTransferMint,
    TransferHookProgramId,
    ConfidentialTransferFeeConfig,
    MetadataPointer,
    GroupPointer,
    GroupMemberPointer,
    ScaledUiAmount,
    Pause,
    PermissionedBurn,
    /// The update authority of token metadata stored in the mint.
    MetadataUpdate,
    /// The update authority of a token group stored in the mint.
    GroupUpdate,
}

impl AuthorityType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Mint => "mint",
            Self::Freeze => "freeze",
            Self::TransferFeeConfig => "transfer-fee-config",
            Self::WithheldWithdraw => "withheld-withdraw",
            Self::CloseMint => "close-mint",
            Self::InterestRate => "interest-rate",
            Self::PermanentDelegate => "permanent-delegate",
            Self::ConfidentialTransferMint => "confidential-transfer-mint",
            Self::TransferHookProgramId => "transfer-hook-program-id",
            Self::ConfidentialTransferFeeConfig => "confidential-transfer-fee-config",
            Self::MetadataPointer => "metadata-pointer",
            Self::GroupPointer => "group-pointer",
            Self::GroupMemberPointer => "group-member-pointer",
            Self::ScaledUiAmount => "scaled-ui-amount",
            Self::Pause => "pause",
            Self::PermissionedBurn => "permissioned-burn",
            Self::MetadataUpdate => "metadata-update",
            Self::GroupUpdate => "group-update",
        }
    }

    /// The Token-2022 extension holding this authority. `None` for the mint and freeze
    /// authorities, which every mint has.
    pub(crate) fn extension(self) -> Option<ExtensionType> {
        match self {
            Self::Mint | Self::Freeze => None,
            Self::TransferFeeConfig | Self::WithheldWithdraw => {
                Some(ExtensionType::TransferFeeConfig)
            }
            Self::CloseMint => Some(ExtensionType::MintCloseAuthority),
            Self::InterestRate => Some(ExtensionType::InterestBearingConfig),
            Self::PermanentDelegate => Some(ExtensionType::PermanentDelegate),
            Self::ConfidentialTransferMint => Some(ExtensionType::ConfidentialTransferMint),
            Self::TransferHookProgramId => Some(ExtensionType::TransferHook),
            Self::ConfidentialTransferFeeConfig => {
                Some(ExtensionType::ConfidentialTransferFeeConfig)
            }
            Self::MetadataPointer => Some(ExtensionType::MetadataPointer),
            Self::GroupPointer => Some(ExtensionType::GroupPointer),
            Self::GroupMemberPointer => Some(ExtensionType::GroupMemberPointer),
            Self::ScaledUiAmount => Some(ExtensionType::ScaledUiAmount),
            Self::Pause => Some(ExtensionType::Pausable),
            Self::PermissionedBurn => Some(ExtensionType::PermissionedBurn),
            Self::MetadataUpdate => Some(ExtensionType::TokenMetadata),
            Self::GroupUpdate => Some(ExtensionType::TokenGroup),
        }
    }

    /// The `SetAuthority` authority type that changes this authority. `None` for the metadata and
    /// group update authorities, which have their own instructions.
    pub(crate) fn token_authority_type(self) -> Option<TokenAuthorityType> {
        Some(match self {
            Self::Mint => TokenAuthorityType::MintTokens,
            Self::Freeze => TokenAuthorityType::FreezeAccount,
            Self::TransferFeeConfig => TokenAuthorityType::TransferFeeConfig,
            Self::WithheldWithdraw => TokenAuthorityType::WithheldWithdraw,
            Self::CloseMint => TokenAuthorityType::CloseMint,
            Self::InterestRate => TokenAuthorityType::InterestRate,
            Self::PermanentDelegate => TokenAuthorityType::PermanentDelegate,
            Self::ConfidentialTransferMint => TokenAuthorityType::ConfidentialTransferMint,
            Self::TransferHookProgramId => TokenAuthorityType::TransferHookProgramId,
            Self::ConfidentialTransferFeeConfig => {
                TokenAuthorityType::ConfidentialTransferFeeConfig
            }
            Self::MetadataPointer => TokenAuthorityType::MetadataPointer,
            Self::GroupPointer => TokenAuthorityType::GroupPointer,
            Self::GroupMemberPointer => TokenAuthorityType::GroupMemberPointer,
            Self::ScaledUiAmount => TokenAuthorityType::ScaledUiAmount,
            Self::Pause => TokenAuthorityType::Pause,
            Self::PermissionedBurn => TokenAuthorityType::PermissionedBurn,
            Self::MetadataUpdate | Self::GroupUpdate => return None,
        })
    }
}

/// Every authority a mint has, keyed by type: its mint and freeze authorities, and the
/// authorities of its Token-2022 extensions. An authority is `None` if it is not set. Authorities
/// of extensions the mint lacks are left out. Works for both Token and Token-2022 mints.
pub(crate) fn authorities(data: &[u8]) -> Result<BTreeMap<AuthorityType, Option<Address>>> {
    let state = StateWithExtensions::<Mint>::unpack(data)?;
    // A missing or malformed extension has no authorities.
    macro_rules! get {
        ($extension:ty) => {
            state.get_extension::<$extension>().ok()
        };
    }
    let transfer_fee = get!(TransferFeeConfig);
    let metadata = state.get_variable_len_extension::<TokenMetadata>().ok();
    Ok(AuthorityType::value_variants()
        .iter()
        .filter_map(|role| {
            let authority = match role {
                AuthorityType::Mint => state.base.mint_authority.into(),
                AuthorityType::Freeze => state.base.freeze_authority.into(),
                AuthorityType::TransferFeeConfig => {
                    transfer_fee?.transfer_fee_config_authority.into()
                }
                AuthorityType::WithheldWithdraw => transfer_fee?.withdraw_withheld_authority.into(),
                AuthorityType::CloseMint => get!(MintCloseAuthority)?.close_authority.into(),
                AuthorityType::InterestRate => get!(InterestBearingConfig)?.rate_authority.into(),
                AuthorityType::PermanentDelegate => get!(PermanentDelegate)?.delegate.into(),
                AuthorityType::ConfidentialTransferMint => {
                    get!(ConfidentialTransferMint)?.authority.into()
                }
                AuthorityType::TransferHookProgramId => get!(TransferHook)?.authority.into(),
                AuthorityType::ConfidentialTransferFeeConfig => {
                    get!(ConfidentialTransferFeeConfig)?.authority.into()
                }
                AuthorityType::MetadataPointer => get!(MetadataPointer)?.authority.into(),
                AuthorityType::GroupPointer => get!(GroupPointer)?.authority.into(),
                AuthorityType::GroupMemberPointer => get!(GroupMemberPointer)?.authority.into(),
                AuthorityType::ScaledUiAmount => get!(ScaledUiAmountConfig)?.authority.into(),
                AuthorityType::Pause => get!(PausableConfig)?.authority.into(),
                AuthorityType::PermissionedBurn => get!(PermissionedBurnConfig)?.authority.into(),
                AuthorityType::MetadataUpdate => metadata.as_ref()?.update_authority.into(),
                AuthorityType::GroupUpdate => get!(TokenGroup)?.update_authority.into(),
            };
            Some((*role, authority))
        })
        .collect())
}
