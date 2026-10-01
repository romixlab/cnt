mod cargo_config;
mod elf;
mod keys;
pub(crate) mod list;
pub(crate) mod logs;
mod output;
mod probe;
mod read;
pub mod reset;
mod run;

use crate::theme::theme;
use anstream::{eprintln, println};
use anyhow::bail;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use cnt_core::{Counters, Storage};
use logs::LogOptions;
pub(crate) use output::Format;
use probe::ProbeOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

    /// Output format of `list`, `read` and `reset`. JSON goes to stdout, diagnostics to stderr
    #[arg(long, global = true, value_enum, default_value_t, env = "CNT_FORMAT")]
    pub format: Format,
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
        /// Keep reading and print counters whose value changed together with defmt logs, until interrupted. All
        /// counters are printed first, lower values are reported as a reset if all counters of the buffer went down,
        /// and as a decrease otherwise
        #[arg(long)]
        watch: bool,
        /// Time between reads in watch mode, in milliseconds
        #[arg(long, default_value_t = 100, requires = "watch")]
        interval: u64,
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        probe: ProbeOptions,
        #[command(flatten)]
        log: LogOptions,
    },
    /// Flash the firmware, restart the target and show defmt logs and counter changes as `read --watch` does, until
    /// interrupted. Can be used as a cargo runner instead of probe-rs: `runner = "cnt run --chip <CHIP>"`
    Run {
        /// Show the TUI instead
        #[arg(long)]
        tui: bool,
        /// Time between counter reads, in milliseconds
        #[arg(long, default_value_t = 100, conflicts_with = "tui")]
        interval: u64,
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        probe: ProbeOptions,
        #[command(flatten)]
        log: LogOptions,
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
        #[command(flatten)]
        log: LogOptions,
    },
}

impl Command {
    pub fn elf(&self) -> &Elf {
        match self {
            Command::List(elf)
            | Command::Read { elf, .. }
            | Command::Run { elf, .. }
            | Command::Tui { elf, .. }
            | Command::Reset { elf, .. } => elf,
        }
    }
}

pub fn process_cmd(
    cmd: Command,
    format: Format,
    mut counters: Counters,
    elf_path: &Path,
) -> anyhow::Result<()> {
    match cmd {
        Command::List(_) => {
            if format == Format::Text {
                list::list(&counters);
            } else {
                output::counters(format, &counters, elf_path)?;
            }
        }
        Command::Read {
            watch: true,
            interval,
            probe,
            log,
            ..
        } => {
            if format == Format::Json {
                bail!("--watch prints a stream of changes, use --format jsonl or text");
            }
            let mut defmt = log.reader(elf_path);
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            read::watch(
                &mut counters,
                &mut core,
                defmt.as_mut(),
                format,
                Duration::from_millis(interval),
            )?;
        }
        Command::Run {
            tui,
            interval,
            probe,
            log,
            ..
        } => {
            if tui && format != Format::Text {
                bail!("The TUI only supports text output");
            }
            if format == Format::Json {
                bail!("run prints a stream of changes, use --format jsonl or text");
            }
            let mut defmt = log.reader(elf_path);
            let mut session = probe.attach(elf_path)?;
            run::flash_and_reset(&mut session, elf_path, defmt.as_ref())?;
            let mut core = session.core(0)?;
            if tui {
                crate::tui::tui(&mut counters, &mut core, defmt)?;
            } else {
                read::watch(
                    &mut counters,
                    &mut core,
                    defmt.as_mut(),
                    format,
                    Duration::from_millis(interval),
                )?;
            }
        }
        Command::Read { probe, .. } => {
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            read::read(&mut counters, &mut core)?;
            if format == Format::Text {
                read::print(&counters);
            } else {
                output::counters(format, &counters, elf_path)?;
            }
        }
        Command::Reset { bkp, probe, .. } => {
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            let hint = theme().hint;
            let mut done = vec![];
            if let Some(block) = counters.ram_counters() {
                if format == Format::Text {
                    println!("{hint}Resetting RAM counters{hint:#}");
                }
                reset::reset(block, &mut core)?;
                done.push(Storage::Ram);
            }
            if bkp && let Some(block) = counters.bkp_counters() {
                if format == Format::Text {
                    println!("{hint}Resetting BKP counters{hint:#}");
                }
                reset::reset(block, &mut core)?;
                done.push(Storage::Bkp);
            }
            if format != Format::Text {
                output::reset(format, &done)?;
            }
        }
        Command::Tui { probe, log, .. } => {
            if format != Format::Text {
                bail!(
                    "The TUI only supports text output, use `cnt read --format json|jsonl` instead"
                );
            }
            let defmt = log.reader(elf_path);
            let mut session = probe.attach(elf_path)?;
            let mut core = session.core(0)?;
            crate::tui::tui(&mut counters, &mut core, defmt)?;
        }
    }
    Ok(())
}
