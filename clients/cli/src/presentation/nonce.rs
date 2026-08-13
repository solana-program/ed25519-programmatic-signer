use {
    crate::{cli::OutputFormat, presentation::render::render},
    anyhow::Result,
    serde::Serialize,
    serde_json::json,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NonceCreateOutput {
    pub(crate) signature: String,
    pub(crate) nonce_account: String,
    pub(crate) authority: String,
    pub(crate) nonce: String,
    pub(crate) lamports: u64,
    pub(crate) rent_lamports: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NonceShowOutput {
    pub(crate) nonce_account: String,
    pub(crate) authority: String,
    pub(crate) nonce: String,
    pub(crate) lamports: u64,
    pub(crate) owner: String,
}

pub(crate) fn render_create(
    output_format: OutputFormat,
    output: NonceCreateOutput,
) -> Result<String> {
    render(
        output_format,
        || {
            format!(
                "signature: {}\nnonce account: {}\nauthority: {}\nnonce: {}\nlamports: {}\nrent \
                 lamports: {}",
                output.signature,
                output.nonce_account,
                output.authority,
                output.nonce,
                output.lamports,
                output.rent_lamports
            )
        },
        || json!(output),
    )
}

pub(crate) fn render_show(output_format: OutputFormat, output: NonceShowOutput) -> Result<String> {
    render(
        output_format,
        || {
            format!(
                "nonce account: {}\nauthority: {}\nnonce: {}\nlamports: {}\nowner: {}",
                output.nonce_account, output.authority, output.nonce, output.lamports, output.owner
            )
        },
        || json!(output),
    )
}
