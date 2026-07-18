use clap::Parser;
use cnt_core::Counters;
use anyhow::Result;

mod cli;
mod tui;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let counters = Counters::load_elf(&cli.elf_path)?;
    cli::process_cmd(cli.command, counters)?;
    Ok(())
}
