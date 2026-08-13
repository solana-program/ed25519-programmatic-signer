fn main() -> anyhow::Result<()> {
    let output = psigner::run_from_args()?;
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(())
}
