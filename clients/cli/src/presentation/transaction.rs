use {
    crate::{cli::OutputFormat, presentation::render::render},
    anyhow::Result,
    serde::Serialize,
    serde_json::json,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VerifyOutput {
    pub(crate) fully_signed: bool,
    pub(crate) transaction_signers: Vec<SignerStatusOutput>,
    pub(crate) genesis_hash: String,
    pub(crate) nonce_account: String,
    pub(crate) nonce_check: Option<NonceCheckOutput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignerStatusOutput {
    pub(crate) address: String,
    pub(crate) signed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NonceCheckOutput {
    pub(crate) source: String,
    pub(crate) nonce: String,
    pub(crate) authority: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubmitOutput {
    pub(crate) signature: String,
    pub(crate) nonce_account: String,
    pub(crate) advanced_nonce: String,
}

pub(crate) fn render_verify(output_format: OutputFormat, output: VerifyOutput) -> Result<String> {
    render(
        output_format,
        || {
            let mut lines = vec![
                format!("fully signed: {}", output.fully_signed),
                format!("genesis hash: {}", output.genesis_hash),
                format!("nonce account: {}", output.nonce_account),
                String::from("transaction signers:"),
            ];
            for signer in &output.transaction_signers {
                lines.push(format!(
                    "  {} {}",
                    signer.address,
                    if signer.signed { "signed" } else { "missing" }
                ));
            }
            if let Some(nonce_check) = &output.nonce_check {
                lines.push(format!("nonce check: {}", nonce_check.source));
                lines.push(format!("  nonce: {}", nonce_check.nonce));
                lines.push(format!("  authority: {}", nonce_check.authority));
            }
            lines.join("\n")
        },
        || json!(output),
    )
}

pub(crate) fn render_submit(output_format: OutputFormat, output: SubmitOutput) -> Result<String> {
    render(
        output_format,
        || {
            format!(
                "signature: {}\nnonce account: {}\nadvanced nonce: {}",
                output.signature, output.nonce_account, output.advanced_nonce
            )
        },
        || json!(output),
    )
}
