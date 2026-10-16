mod cargo_config;
mod elf_cmd;
mod logs;
mod probe;
mod run;
mod target;

use anstream::{eprintln, println};
use anyhow::bail;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use cnt_core::{Counters, Storage};
use cnt_view::{Format, output, theme};
use elf_cmd::ElfCommand;
use logs::LogOptions;
use probe::ProbeOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;
use target::ProbeSource;

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_SHA"),
    ", commit ",
    env!("BUILD_TIME"),
    ")"
);

#[derive(Parser)]
#[command(version = VERSION)]
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
    /// directory is used, or the ELF with the build id the target runs (target/, WW_ELF_PATH, ~/.cache/ww/elf)
    elf_path: Option<PathBuf>,
}

impl Elf {
    /// The ELF path given on the command line, or the newest one built in the current cargo project.
    pub fn resolve(&self) -> anyhow::Result<PathBuf> {
        match &self.elf_path {
            Some(path) => Ok(path.clone()),
            None => {
                let path = cnt_core::elf::newest_target_binary()?;
                let (hint, style) = (theme().hint, theme().path);
                eprintln!("{hint}Using ELF{hint:#} {style}{}{style:#}", path.display());
                Ok(path)
            }
        }
    }

    fn given(&self) -> bool {
        self.elf_path.is_some()
    }
}

#[derive(Args)]
pub(crate) struct Check {
    /// Use the ELF although the target runs other firmware (its build id differs)
    #[arg(long)]
    force: bool,
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
        check: Check,
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
        check: Check,
        #[command(flatten)]
        probe: ProbeOptions,
    },
    /// Run terminal UI
    Tui {
        #[command(flatten)]
        elf: Elf,
        #[command(flatten)]
        check: Check,
        #[command(flatten)]
        probe: ProbeOptions,
        #[command(flatten)]
        log: LogOptions,
    },
    /// The ELF directory, where the ELF of the firmware a target runs is found by its build id
    #[command(subcommand)]
    Elf(ElfCommand),
}

impl Command {
    fn elf(&self) -> Option<&Elf> {
        match self {
            Command::List(elf)
            | Command::Read { elf, .. }
            | Command::Run { elf, .. }
            | Command::Tui { elf, .. }
            | Command::Reset { elf, .. } => Some(elf),
            Command::Elf(_) => None,
        }
    }
}

pub fn process_cmd(cmd: Command, format: Format) -> anyhow::Result<()> {
    let Some(elf) = cmd.elf() else {
        let Command::Elf(cmd) = cmd else {
            unreachable!()
        };
        return elf_cmd::run(cmd);
    };
    let given = elf.given();
    let mut elf_path = elf.resolve()?;
    let mut counters = Counters::load_elf(&elf_path)?;
    match cmd {
        Command::List(_) => {
            if format == Format::Text {
                cnt_view::list::list(&counters);
            } else {
                output::counters(format, &counters, &elf_path)?;
            }
        }
        Command::Read {
            watch: true,
            interval,
            check,
            probe,
            log,
            ..
        } => {
            if format == Format::Json {
                bail!("--watch prints a stream of changes, use --format jsonl or text");
            }
            let mut session = probe.attach(&elf_path)?;
            let mut core = session.core(0)?;
            checked(&mut core, &mut counters, &mut elf_path, given, check.force)?;
            let defmt = log.reader(&elf_path);
            let mut source = ProbeSource::new(core, &counters, defmt);
            cnt_view::watch(
                &mut counters,
                &mut source,
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
            let defmt = log.reader(&elf_path);
            let mut session = probe.attach(&elf_path)?;
            // Runs the firmware just flashed, nothing to check
            run::flash_and_reset(&mut session, &elf_path, defmt.as_ref())?;
            keep_elf(&elf_path);
            let core = session.core(0)?;
            let mut source = ProbeSource::new(core, &counters, defmt);
            if tui {
                cnt_view::tui(&mut counters, &mut source, "")?;
            } else {
                cnt_view::watch(
                    &mut counters,
                    &mut source,
                    format,
                    Duration::from_millis(interval),
                )?;
            }
        }
        Command::Read { check, probe, .. } => {
            let mut session = probe.attach(&elf_path)?;
            let mut core = session.core(0)?;
            checked(&mut core, &mut counters, &mut elf_path, given, check.force)?;
            let mut source = ProbeSource::new(core, &counters, None);
            counters.read(&mut source)?;
            if format == Format::Text {
                cnt_view::print(&counters);
            } else {
                output::counters(format, &counters, &elf_path)?;
            }
        }
        Command::Reset {
            bkp, check, probe, ..
        } => {
            let mut session = probe.attach(&elf_path)?;
            let mut core = session.core(0)?;
            checked(&mut core, &mut counters, &mut elf_path, given, check.force)?;
            let mut source = ProbeSource::new(core, &counters, None);
            let hint = theme().hint;
            let mut done = vec![];
            for storage in [Storage::Ram, Storage::Bkp] {
                let in_use = match storage {
                    Storage::Ram => counters.ram_counters().is_some(),
                    Storage::Bkp => bkp && counters.bkp_counters().is_some(),
                };
                if !in_use {
                    continue;
                }
                if format == Format::Text {
                    println!("{hint}Resetting {storage} counters{hint:#}");
                }
                counters.reset(storage, &mut source)?;
                done.push(storage);
            }
            if format != Format::Text {
                output::reset(format, &done)?;
            }
        }
        Command::Tui {
            check, probe, log, ..
        } => {
            if format != Format::Text {
                bail!(
                    "The TUI only supports text output, use `cnt read --format json|jsonl` instead"
                );
            }
            let mut session = probe.attach(&elf_path)?;
            let mut core = session.core(0)?;
            checked(&mut core, &mut counters, &mut elf_path, given, check.force)?;
            let defmt = log.reader(&elf_path);
            let mut source = ProbeSource::new(core, &counters, defmt);
            cnt_view::tui(&mut counters, &mut source, "")?;
        }
        Command::Elf(_) => unreachable!(),
    }
    Ok(())
}

/// [`target::check_firmware`], switching to the ELF with the target's build id if it found one.
fn checked(
    core: &mut probe_rs::Core,
    counters: &mut Counters,
    elf_path: &mut PathBuf,
    given: bool,
    force: bool,
) -> anyhow::Result<()> {
    if let Some(found) = target::check_firmware(core, counters, elf_path, given, force)? {
        *counters = Counters::load_elf(&found)?;
        *elf_path = found;
    }
    Ok(())
}

/// Store the ELF just flashed by its build id (`cnt_core::elf::add`), so `counters`/`log` can find it by build id
/// later, on this machine or (if a shared directory is configured) another one, without `--elf`. The flash already
/// succeeded, so a failure here (e.g. no cache directory) is a warning, not an error.
fn keep_elf(elf_path: &Path) {
    let Some(dir) = cnt_core::elf::default_write_dir() else {
        return;
    };
    if let Err(e) = cnt_core::elf::add(elf_path, &dir) {
        let warn = theme().warn;
        eprintln!("⚠️ {warn}Could not cache the flashed ELF: {e}{warn:#}");
    }
}
