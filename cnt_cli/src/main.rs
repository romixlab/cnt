use cnt_core::Counters;
use anyhow::Result;

mod cli;
mod tui;

fn main() -> Result<()> {
    let cli = cli::Cli::parse_styled();
    let counters = Counters::load_elf(cli.command.elf_path())?;
    cli::process_cmd(cli.command, counters)?;
    Ok(())
}
