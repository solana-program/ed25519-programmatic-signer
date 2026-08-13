use {
    anyhow::{Context, Result, anyhow, bail},
    solana_address::Address,
    solana_derivation_path::DerivationPath,
    solana_keypair::{Keypair, read_keypair_file},
    solana_remote_wallet::{
        locator::Locator,
        remote_keypair::generate_remote_keypair,
        remote_wallet::{RemoteWalletManager, maybe_wallet_manager},
    },
    solana_signer::Signer,
    spl_ed25519_signer_interface::pda::ProgrammaticSigner,
    std::{
        path::{Path, PathBuf},
        rc::Rc,
    },
    uriparse::URIReference,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SignerSource {
    File(PathBuf),
    RemoteWallet {
        locator: Locator,
        derivation_path: DerivationPath,
    },
}

pub(crate) fn read_keypair(path: &Path) -> Result<Keypair> {
    read_keypair_file(path)
        .map_err(|error| anyhow!("failed to read keypair {}: {error}", path.display()))
}

pub(crate) fn read_signer(
    source: &str,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Box<dyn Signer>> {
    match parse_signer_source(source)? {
        SignerSource::File(path) => {
            let keypair = read_keypair(&path)?;
            Ok(Box::new(keypair))
        }
        SignerSource::RemoteWallet {
            locator,
            derivation_path,
        } => {
            if wallet_manager.is_none() {
                *wallet_manager =
                    maybe_wallet_manager().context("failed to initialize remote wallet manager")?;
            }
            let Some(wallet_manager) = wallet_manager.as_ref() else {
                bail!("no remote wallet found for signer source {source}");
            };
            let signer = generate_remote_keypair(
                locator,
                derivation_path,
                wallet_manager.as_ref(),
                false,
                keypair_name,
            )
            .with_context(|| format!("failed to load remote wallet signer {source}"))?;
            Ok(Box::new(signer))
        }
    }
}

pub(crate) fn read_signers(
    sources: &[String],
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Vec<Box<dyn Signer>>> {
    sources
        .iter()
        .map(|source| read_signer(source, keypair_name, wallet_manager))
        .collect()
}

pub(crate) fn programmatic_signer(authority: &Address) -> Address {
    ProgrammaticSigner::derive_address(&spl_ed25519_signer_interface::id(), authority)
}

pub(crate) fn signer_refs(signers: &[Box<dyn Signer>]) -> Vec<&dyn Signer> {
    signers.iter().map(|signer| signer.as_ref()).collect()
}

pub(crate) fn parse_signer_source(source: &str) -> Result<SignerSource> {
    if !source.contains("://") {
        return Ok(SignerSource::File(PathBuf::from(source)));
    }

    let uri = URIReference::try_from(source)
        .with_context(|| format!("invalid signer source {source}"))?;
    let Some(scheme) = uri
        .scheme()
        .map(|scheme| scheme.as_str().to_ascii_lowercase())
    else {
        return Ok(SignerSource::File(PathBuf::from(source)));
    };
    match scheme.as_str() {
        "usb" => {
            let locator = Locator::new_from_uri(&uri)
                .with_context(|| format!("invalid remote wallet signer source {source}"))?;
            let derivation_path = DerivationPath::from_uri_key_query(&uri)
                .with_context(|| format!("invalid derivation path in signer source {source}"))?
                .unwrap_or_default();
            Ok(SignerSource::RemoteWallet {
                locator,
                derivation_path,
            })
        }
        "file" => Ok(SignerSource::File(PathBuf::from(uri.path().to_string()))),
        _ => bail!("unsupported signer URL scheme `{scheme}` in {source}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_signer_sources() {
        assert_eq!(
            parse_signer_source("authority.json").unwrap(),
            SignerSource::File(PathBuf::from("authority.json"))
        );
    }

    #[test]
    fn parses_ledger_signer_urls() {
        let SignerSource::RemoteWallet {
            locator,
            derivation_path,
        } = parse_signer_source("usb://ledger?key=0/0").unwrap()
        else {
            panic!("expected remote wallet source");
        };
        assert_eq!(locator.to_string(), "usb://ledger/");
        assert_eq!(derivation_path.get_query(), "?key=0'/0'");
    }

    #[test]
    fn rejects_unsupported_signer_urls() {
        let error = parse_signer_source("https://example.com/keypair")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported signer URL scheme"));
    }

    #[test]
    fn rejects_invalid_ledger_derivation_paths() {
        let error = parse_signer_source("usb://ledger?key=0/0/0")
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid derivation path"));
    }
}
