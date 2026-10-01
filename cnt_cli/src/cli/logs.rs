//! defmt logs, read over RTT and decoded with the table from the firmware ELF.
//!
//! RTT access comes from `probe-rs` and decoding from `defmt-decoder`, the libraries `probe-rs run` is built on. Only
//! the glue is here, so that logs can be read through the same probe session as the counters: a probe can only be
//! opened by one process at a time.

use crate::cli::list::severity_label;
use crate::theme::theme;
use anstream::eprintln;
use anyhow::Context;
use clap::Args;
use cnt_core::Severity;
use defmt_decoder::{DecodeError, Location, Locations, StreamDecoder, Table};
use defmt_parser::Level;
use probe_rs::Core;
use probe_rs::rtt::{self, ChannelMode, Rtt, ScanRegion};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often to look for the RTT control block while the firmware has not set it up yet.
const ATTACH_INTERVAL: Duration = Duration::from_millis(200);
/// Name of the RTT up channel used by `defmt-rtt`.
const DEFMT_CHANNEL: &str = "defmt";

#[derive(Args)]
#[command(next_help_heading = "Log Options")]
pub(crate) struct LogOptions {
    /// Do not show defmt logs. By default they are shown in watch mode, in the TUI and by `run` if the firmware uses
    /// defmt-rtt
    #[arg(long)]
    no_defmt: bool,
}

impl LogOptions {
    /// A reader for the defmt logs of the firmware, or `None` if disabled or the firmware does not log over RTT.
    ///
    /// Problems with the defmt data are reported, but do not prevent reading counters.
    pub fn reader(&self, elf_path: &Path) -> Option<DefmtReader> {
        if self.no_defmt {
            return None;
        }
        let warn = theme().warn;
        let elf = match std::fs::read(elf_path) {
            Ok(elf) => elf,
            Err(e) => {
                eprintln!("⚠️ {warn}Failed to read the ELF for defmt logs: {e}{warn:#}");
                return None;
            }
        };
        DefmtReader::new(&elf).unwrap_or_else(|e| {
            eprintln!("⚠️ {warn}defmt logs are not shown: {e:#}{warn:#}");
            None
        })
    }
}

/// A decoded log message.
pub struct Log {
    /// `None` for `defmt::println!`
    pub level: Option<Level>,
    /// Target timestamp, if the firmware defines one with `defmt::timestamp!`
    pub timestamp: Option<String>,
    pub message: String,
    pub location: Option<&'static Location>,
}

impl Log {
    /// The counter severity with the same name, for styling.
    pub fn severity(&self) -> Option<Severity> {
        Some(match self.level? {
            Level::Trace => Severity::Trace,
            Level::Debug => Severity::Debug,
            Level::Info => Severity::Info,
            Level::Warn => Severity::Warn,
            Level::Error => Severity::Error,
        })
    }

    /// Fixed-width level label, blank for `println!`.
    pub fn level_label(&self) -> &'static str {
        self.severity().map_or("     ", severity_label)
    }

    /// `file:line`, with the file relative to the current directory if it is below it.
    pub fn file_line(&self) -> Option<String> {
        let loc = self.location?;
        let file = std::env::current_dir()
            .ok()
            .and_then(|cwd| loc.file.strip_prefix(cwd).ok().map(PathBuf::from))
            .unwrap_or_else(|| loc.file.clone());
        Some(format!("{}:{}", file.display(), loc.line))
    }
}

/// Parsed defmt data of the firmware.
struct Elf {
    table: Table,
    /// `None` if the debug info does not cover every log statement
    locations: Option<Locations>,
}

/// Reads defmt logs from the target over RTT.
///
/// While attached, the defmt channel is switched to blocking mode, so that the firmware waits for logs to be read instead
/// of dropping them, as `probe-rs run` does. Call [`DefmtReader::detach`] when done, or the firmware stops once the RTT
/// buffer is full.
pub struct DefmtReader {
    elf: &'static Elf,
    decoder: Box<dyn StreamDecoder + Send + Sync>,
    /// Address of the RTT control block, `_SEGGER_RTT`
    control_block: u64,
    channel: Option<Channel>,
    last_attach: Option<Instant>,
    buf: Vec<u8>,
}

/// The defmt RTT channel of an attached control block.
struct Channel {
    rtt: Rtt,
    number: usize,
    /// Mode set by the firmware, restored when detaching
    original_mode: ChannelMode,
}

impl DefmtReader {
    /// `None` if the ELF has no defmt data.
    fn new(elf: &[u8]) -> anyhow::Result<Option<Self>> {
        let Some(table) = Table::parse(elf).context("Failed to parse defmt data")? else {
            return Ok(None);
        };
        let Some(control_block) = rtt::find_rtt_control_block_in_raw_file(elf).ok().flatten()
        else {
            anyhow::bail!("the ELF has defmt data, but no RTT control block (_SEGGER_RTT)");
        };
        let locations = table
            .get_locations(elf)
            .context("Failed to parse defmt locations")?;
        let locations = if table.indices().all(|i| locations.contains_key(&(i as u64))) {
            Some(locations)
        } else {
            let warn = theme().warn;
            eprintln!(
                "⚠️ {warn}defmt location info is incomplete, compile with `debug = 2` to show it{warn:#}"
            );
            None
        };
        // Lives until the process exits, the stream decoder borrows the table
        let elf: &'static Elf = Box::leak(Box::new(Elf { table, locations }));
        Ok(Some(DefmtReader {
            elf,
            decoder: elf.table.new_stream_decoder(),
            control_block,
            channel: None,
            last_attach: None,
            buf: vec![],
        }))
    }

    /// Address of the RTT control block.
    pub fn control_block(&self) -> u64 {
        self.control_block
    }

    pub fn is_attached(&self) -> bool {
        self.channel.is_some()
    }

    /// Read and decode the logs sent since the last call.
    ///
    /// Attaches to RTT once the firmware has set it up, and again after the target was reset.
    pub fn poll(&mut self, core: &mut Core) -> anyhow::Result<Vec<Log>> {
        let mut logs = vec![];
        if !self.try_attach(core)? {
            return Ok(logs);
        }
        while let Some(ch) = &mut self.channel {
            let up = ch
                .rtt
                .up_channel(ch.number)
                .expect("channel checked on attach");
            let n = match up.read(core, &mut self.buf) {
                Ok(n) => n,
                // The target was reset or re-initialised RTT
                Err(rtt::Error::ControlBlockCorrupted(_) | rtt::Error::ReadPointerChanged) => {
                    self.channel = None;
                    break;
                }
                Err(e) => return Err(e).context("Failed to read defmt logs"),
            };
            if n == 0 {
                break;
            }
            self.decoder.received(&self.buf[..n]);
            self.decode(&mut logs);
            if n < self.buf.len() {
                break;
            }
        }
        Ok(logs)
    }

    fn decode(&mut self, logs: &mut Vec<Log>) {
        let elf = self.elf;
        loop {
            match self.decoder.decode() {
                Ok(frame) => logs.push(Log {
                    level: frame.level(),
                    timestamp: frame.display_timestamp().map(|t| t.to_string()),
                    message: frame.display_message().to_string(),
                    location: elf
                        .locations
                        .as_ref()
                        .and_then(|locs| locs.get(&frame.index())),
                }),
                Err(DecodeError::UnexpectedEof) => break,
                // rzCOBS (the default) resynchronises on the next frame
                Err(DecodeError::Malformed) if elf.table.encoding().can_recover() => {}
                Err(DecodeError::Malformed) => {
                    // Raw encoding cannot find the next frame, start over with the next data read
                    self.decoder = elf.table.new_stream_decoder();
                    logs.push(Log {
                        level: Some(Level::Error),
                        timestamp: None,
                        message: "cnt: malformed defmt data, some logs were lost".into(),
                        location: None,
                    });
                    break;
                }
            }
        }
    }

    fn try_attach(&mut self, core: &mut Core) -> anyhow::Result<bool> {
        if self.channel.is_some() {
            return Ok(true);
        }
        if self
            .last_attach
            .is_some_and(|t| t.elapsed() < ATTACH_INTERVAL)
        {
            return Ok(false);
        }
        self.last_attach = Some(Instant::now());

        let mut rtt = match Rtt::attach_region(core, &ScanRegion::Exact(self.control_block)) {
            Ok(rtt) => rtt,
            // Not set up by the firmware yet, e.g. right after a reset
            Err(rtt::Error::ControlBlockNotFound | rtt::Error::ControlBlockCorrupted(_)) => {
                return Ok(false);
            }
            Err(e) => return Err(e).context("Failed to attach to RTT"),
        };
        let Some(number) = rtt
            .up_channels()
            .iter()
            .find(|c| c.name() == Some(DEFMT_CHANNEL))
            .map(|c| c.number())
        else {
            return Ok(false);
        };
        let up = rtt.up_channel(number).expect("channel just found");
        let original_mode = up.mode(core)?;
        up.set_mode(core, ChannelMode::BlockIfFull)?;
        self.buf = vec![0; up.buffer_size().max(64)];
        self.channel = Some(Channel {
            rtt,
            number,
            original_mode,
        });
        Ok(true)
    }

    /// Restore the channel mode set by the firmware, so that it does not block once nobody reads the logs.
    pub fn detach(&mut self, core: &mut Core) -> anyhow::Result<()> {
        if let Some(mut ch) = self.channel.take()
            && let Some(up) = ch.rtt.up_channel(ch.number)
        {
            up.set_mode(core, ch.original_mode)
                .context("Failed to restore the defmt RTT channel mode")?;
        }
        Ok(())
    }
}
