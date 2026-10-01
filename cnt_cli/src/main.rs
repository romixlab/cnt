use anyhow::Result;
use cnt_core::Counters;

mod cli;
mod theme;
mod tui;

fn main() -> Result<()> {
    let cli = cli::Cli::parse_styled();
    let elf_path = cli.command.elf().resolve()?;
    let counters = Counters::load_elf(&elf_path)?;
    cli::process_cmd(cli.command, cli.format, counters, &elf_path)?;
    Ok(())
}
