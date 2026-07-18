use clap::Parser;
use cnt_core::Counters;
use anyhow::Result;

mod cli;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let counters = Counters::load_elf(&cli.elf_path);
    Ok(())
}
