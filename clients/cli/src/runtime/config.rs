use {
    anyhow::{Context, Result, anyhow},
    std::{env, fs, path::PathBuf},
};

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8899";

pub(crate) fn resolve_url(argument: Option<&str>) -> Result<String> {
    if let Some(argument) = argument {
        return Ok(url_from_moniker(argument));
    }
    if let Ok(url) = env::var("SOLANA_URL") {
        return Ok(url_from_moniker(&url));
    }
    if let Some(url) = solana_config_value("json_rpc_url")? {
        return Ok(url_from_moniker(&url));
    }
    Ok(String::from(DEFAULT_RPC_URL))
}

pub(crate) fn keypair_source(argument: Option<&str>) -> Result<String> {
    if let Some(source) = argument {
        return Ok(source.to_string());
    }
    if let Ok(source) = env::var("SOLANA_KEYPAIR") {
        return Ok(source);
    }
    solana_config_value("keypair_path")?.ok_or_else(|| {
        anyhow!("keypair source is required; pass --fee-payer or set Solana CLI config")
    })
}

fn url_from_moniker(value: &str) -> String {
    match value {
        "l" | "localhost" | "localnet" => String::from(DEFAULT_RPC_URL),
        "m" | "mainnet" | "mainnet-beta" => String::from("https://api.mainnet-beta.solana.com"),
        "d" | "devnet" => String::from("https://api.devnet.solana.com"),
        "t" | "testnet" => String::from("https://api.testnet.solana.com"),
        _ => value.to_string(),
    }
}

fn solana_config_value(key: &str) -> Result<Option<String>> {
    let Some(config_path) = solana_config_path() else {
        return Ok(None);
    };
    if !config_path.exists() {
        return Ok(None);
    }
    let config = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    for line in config.lines() {
        let trimmed = line.trim();
        let Some(value) = trimmed
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            continue;
        };
        return Ok(Some(trim_config_value(value)));
    }
    Ok(None)
}

fn solana_config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("SOLANA_CONFIG") {
        return Some(PathBuf::from(path));
    }
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("solana")
            .join("cli")
            .join("config.yml"),
    )
}

fn trim_config_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}
