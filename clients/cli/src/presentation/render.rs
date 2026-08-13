use {
    crate::cli::OutputFormat,
    anyhow::{Context, Result},
    serde_json::Value,
};

pub(crate) fn render(
    output: OutputFormat,
    display: impl FnOnce() -> String,
    value: impl FnOnce() -> Value,
) -> Result<String> {
    match output {
        OutputFormat::Display => Ok(display()),
        OutputFormat::Json => {
            serde_json::to_string_pretty(&value()).context("failed to encode JSON")
        }
        OutputFormat::JsonCompact => {
            serde_json::to_string(&value()).context("failed to encode JSON")
        }
    }
}
