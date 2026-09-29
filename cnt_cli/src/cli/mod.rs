mod cargo_config;
mod elf;
mod list;
mod probe;
mod read;
pub mod reset;

use crate::theme::theme;
use anstream::{eprintln, println};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use cnt_core::Counters;
use probe::ProbeOptions;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version)]
#[command(propagate_version = true)]
#[command(color = clap::ColorChoice::Auto)]
#[command(
    about = "Command line interface for the embedded counters crate. https://crates.io/crates/cnt"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Apply the colour theme and parse arguments.
    ///
    /// The theme is downgraded at runtime depending on the terminal's capabilities,
    /// so it is applied here rather than via `#[command(styles = ...)]`.
    pub fn parse_styled() -> Self {
        let matches = Self::command().styles(theme().clap.clone()).get_matches();
        match Self::from_arg_matches(&matches) {
            Ok(cli) => cli,
            Err(err) => err.exit(),
        }
    }
}

#[derive(Args)]
pub(crate) struct Elf {
    /// Path to the firmware ELF file. If omitted, the most recently built binary of the cargo project in the current
    /// directory is used
    elf_path: Option<PathBuf>,
}

impl Elf {
    /// The ELF path given on the command line, or the one found in the current cargo project.
    pub fn resolve(&self) -> anyhow::Result<PathBuf> {
        match &self.elf_path {
            Some(path) => Ok(path.clone()),
            None => {
                let path = elf::find_elf()?;
                let (hint, style) = (theme().hint, theme().path);
                eprintln!("{hint}Using ELF{hint:#} {style}{}{style:#}", path.display());
                Ok(path)
            }
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// List counters
    List(Elf),
    /// Read counters from a connected target using probe-rs
    Read {
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        probe: ProbeOptions,
    },
    /// Reset counters to zero on a connected target
    Reset {
        /// Reset BKP counters as well if they are in use
        #[clap(default_value = "false", long)]
        bkp: bool,
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        probe: ProbeOptions,
    },
    /// Run terminal UI
    Tui {
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        probe: ProbeOptions,
    },
}

impl Command {
    pub fn elf(&self) -> &Elf {
        match self {
            Command::List(elf)
            | Command::Read { elf, .. }
            | Command::Tui { elf, .. }
            | Command::Reset { elf, .. } => elf,
        }
    }
}

pub fn process_cmd(cmd: Command, mut counters: Counters, elf_path: &Path) -> anyhow::Result<()> {
    match cmd {
        Command::List(_) => {
            list::list(&counters);
        }
        Command::Read { probe, .. } => {
            let mut session = probe.attach(elf_path)?;
            let core = session.core(0)?;
            read::read(&mut counters, core)?;
        }
        Command::Reset { bkp, probe, .. } => {
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            let hint = theme().hint;
            if let Some(block) = counters.ram_counters() {
                println!("{hint}Resetting RAM counters{hint:#}");
                reset::reset(&block, &mut core)?;
            }
            if bkp && let Some(block) = counters.bkp_counters() {
                println!("{hint}Resetting BKP counters{hint:#}");
                reset::reset(&block, &mut core)?;
            }
        }
        Command::Tui { probe, .. } => {
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            crate::tui::tui(&mut counters, &mut core)?;
        }
    }
    Ok(())
}
