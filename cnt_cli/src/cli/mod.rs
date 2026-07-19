mod list;
mod read;
pub mod reset;

use clap::{Parser, Subcommand};
use cnt_core::Counters;
use probe_rs::probe::list::Lister;
use probe_rs::{Permissions, Session};
use std::path::PathBuf;

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
