use {
    anyhow::{Context, Result},
    clap::ValueEnum,
    serde::Serialize,
    std::fmt::Display,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Display,
    Json,
    JsonCompact,
}

impl OutputFormat {
    pub(crate) fn render(self, output: &(impl Display + Serialize)) -> Result<String> {
        match self {
            Self::Display => Ok(output.to_string()),
            Self::Json => serde_json::to_string_pretty(output).context("failed to encode JSON"),
            Self::JsonCompact => serde_json::to_string(output).context("failed to encode JSON"),
        }
    }
}
