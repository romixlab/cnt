use anyhow::Result;

mod cli;

fn main() -> Result<()> {
    let cli = cli::Cli::parse_styled();
    cli::process_cmd(cli.command, cli.format)
}
