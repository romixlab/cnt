mod list;

use std::path::PathBuf;
use clap::{Parser, Subcommand};
use cnt_core::Counters;

/// Command line interface for the embedded counters crate.
/// https://crates.io/crates/cnt
///
#[derive(Parser)]
// #[command(version, about, long_about = None)]
// #[command(propagate_version = true)]
// #[command(color = clap::ColorChoice::Auto)]
pub struct Cli {
    pub elf_path: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// List counters
    List,
    /// Run terminal UI
    Tui,
}

pub fn process_cmd(cmd: Command, counters: Counters) -> anyhow::Result<()> {
    match cmd {
        Command::List => {
            list::list(&counters);
        }
        Command::Tui => {}
    }
    Ok(())
}
