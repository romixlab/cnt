mod list;
mod read;
pub mod reset;
mod theme;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use cnt_core::Counters;
use probe_rs::probe::list::Lister;
use probe_rs::{Permissions, Session};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version)]
#[command(propagate_version = true)]
#[command(color = clap::ColorChoice::Auto)]
#[command(
    about = "Command line interface for the embedded counters crate. https://crates.io/crates/cnt"
)]
pub struct Cli {
    pub elf_path: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Apply the colour theme and parse arguments.
    ///
    /// The theme is downgraded at runtime depending on the terminal's capabilities,
    /// so it is applied here rather than via `#[command(styles = ...)]`.
    pub fn parse_styled() -> Self {
        let matches = Self::command().styles(theme::select_style()).get_matches();
        match Self::from_arg_matches(&matches) {
            Ok(cli) => cli,
            Err(err) => err.exit(),
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// List counters
    List,
    /// Read counters from a connected target using probe-rs
    Read,
    /// Reset counters to zero on a connected target
    Reset {
        /// Reset BKP counters as well if they are in use
        #[clap(default_value = "false", long)]
        bkp: bool,
    },
    /// Run terminal UI
    Tui,
}

pub fn process_cmd(cmd: Command, mut counters: Counters) -> anyhow::Result<()> {
    match cmd {
        Command::List => {
            list::list(&counters);
        }
        Command::Read => {
            let mut session = connect_probe()?;
            let core = session.core(0)?;
            read::read(&mut counters, core)?;
        }
        Command::Reset { bkp } => {
            let mut session = connect_probe()?;
            let mut core = session.core(0)?;
            if let Some(block) = counters.ram_counters() {
                println!("Resetting RAM counters");
                reset::reset(&block, &mut core)?;
            }
            if bkp && let Some(block) = counters.bkp_counters() {
                println!("Resetting BKP counters");
                reset::reset(&block, &mut core)?;
            }
        }
        Command::Tui => {
            let mut session = connect_probe()?;
            let mut core = session.core(0)?;
            crate::tui::tui(&mut counters, &mut core)?;
        }
    }
    Ok(())
}

fn connect_probe() -> anyhow::Result<Session> {
    let lister = Lister::new();
    let probes = lister.list_all();
    let probe = probes[0].open()?;
    let session = probe.attach("STM32H533RE", Permissions::default())?;
    Ok(session)
}
