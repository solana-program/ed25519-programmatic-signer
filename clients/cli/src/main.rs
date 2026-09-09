//! Command-line entrypoint.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    spl_programmatic_signer_cli::run().await
}
