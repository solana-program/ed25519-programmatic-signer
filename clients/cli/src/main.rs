mod cli;
mod commands;
mod output;

use {anyhow::Result, clap::Parser, cli::Cli};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let output = commands::run(cli)?;
    println!("{output}");
    Ok(())
}
