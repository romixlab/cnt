//! `cnt run`: flash the firmware and restart the target, like `probe-rs run`.

use super::logs::DefmtReader;
use crate::theme::theme;
use anstream::{eprint, eprintln};
use anyhow::Context;
use human_repr::HumanCount;
use probe_rs::Session;
use probe_rs::flashing::{
    DownloadOptions, ElfLoader, FlashProgress, ProgressEvent, ProgressOperation, build_loader,
};
use probe_rs::rtt::{Rtt, ScanRegion};
use std::io::IsTerminal;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long to wait for the core to halt after reset.
const RESET_TIMEOUT: Duration = Duration::from_millis(500);

/// Flash the ELF and reset the target, so that it runs the new firmware from the start.
pub fn flash_and_reset(
    session: &mut Session,
    elf_path: &Path,
    defmt: Option<&DefmtReader>,
) -> anyhow::Result<()> {
    let t = theme();
    let (hint, path) = (t.hint, t.path);
    eprintln!(
        "{hint}Flashing{hint:#} {path}{}{path:#}",
        elf_path.display()
    );
    let loader = build_loader(session, elf_path, ElfLoader(Default::default()), None)
        .context("Failed to load the ELF")?;

    let mut progress = Progress::new();
    let mut options = DownloadOptions::new();
    options.progress = FlashProgress::new(|event| progress.update(event));
    let flashed = loader.commit(session, options);
    progress.clear();
    flashed.context("Failed to flash the firmware")?;

    let mut core = session.core(0)?;
    core.reset_and_halt(RESET_TIMEOUT)
        .context("Failed to reset the target")?;
    // RAM still has the control block of the previous run, attaching to it would read stale logs. Unless the ELF
    // places it with its initial contents, then it is valid already.
    if let Some(defmt) = defmt
        && !loader.has_data_for_address(defmt.control_block())
    {
        Rtt::clear_control_block(&mut core, &ScanRegion::Exact(defmt.control_block()))
            .context("Failed to clear the RTT control block")?;
    }
    core.run().context("Failed to start the target")?;
    Ok(())
}

/// Width of the progress bar, in characters.
const BAR_WIDTH: usize = 30;

/// Flashing progress on stderr: a bar for the running operation, replaced by a summary line once it finishes. Only the
/// summary lines if stderr is not a terminal.
struct Progress {
    tty: bool,
    /// Bytes to process per operation, `None` if unknown (whole chip erase)
    totals: [Option<u64>; 4],
    done: [u64; 4],
    started: Instant,
    /// Last drawn operation and percentage, to redraw only when it changes
    drawn: Option<(usize, u64)>,
}

impl Progress {
    fn new() -> Self {
        Progress {
            tty: std::io::stderr().is_terminal(),
            totals: [Some(0); 4],
            done: [0; 4],
            started: Instant::now(),
            drawn: None,
        }
    }

    fn update(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::AddProgressBar { operation, total } => {
                let i = index(operation);
                self.totals[i] = self.totals[i].zip(total).map(|(a, b)| a + b);
            }
            ProgressEvent::Started(op) => {
                self.started = Instant::now();
                self.draw(op);
            }
            ProgressEvent::Progress {
                operation, size, ..
            } => {
                self.done[index(operation)] += size;
                self.draw(operation);
            }
            ProgressEvent::Finished(op) => {
                self.clear();
                let hint = theme().hint;
                let size = match self.done[index(op)] {
                    0 => String::new(),
                    n => format!(" {}", n.human_count_bytes()),
                };
                eprintln!(
                    "{hint}  {}{size} in {:.2}s{hint:#}",
                    done(op),
                    self.started.elapsed().as_secs_f64()
                );
            }
            ProgressEvent::Failed(_) => self.clear(),
            _ => {}
        }
    }

    /// `  Programming [████████░░░░░░░░]  45% 58 kB / 128 kB`
    fn draw(&mut self, op: ProgressOperation) {
        if !self.tty {
            return;
        }
        let i = index(op);
        let done = self.done[i];
        let Some(total) = self.totals[i].filter(|&t| t > 0) else {
            if self.drawn != Some((i, u64::MAX)) {
                self.drawn = Some((i, u64::MAX));
                let hint = theme().hint;
                eprint!("\r\x1b[2K{hint}  {}...{hint:#}", running(op));
            }
            return;
        };
        let percent = (done * 100 / total).min(100);
        if self.drawn == Some((i, percent)) {
            return;
        }
        self.drawn = Some((i, percent));
        let t = theme();
        let (hint, accent) = (t.hint, t.table);
        let filled = percent as usize * BAR_WIDTH / 100;
        eprint!(
            "\r\x1b[2K{hint}  {:<11}{hint:#} {accent}{}{accent:#}{hint}{}{hint:#} {percent:>3}% {hint}{} / {}{hint:#}",
            running(op),
            "█".repeat(filled),
            "░".repeat(BAR_WIDTH - filled),
            done.human_count_bytes(),
            total.human_count_bytes(),
        );
    }

    /// Remove the bar, so that the next line starts on a clean line.
    fn clear(&mut self) {
        if self.drawn.take().is_some() {
            eprint!("\r\x1b[2K");
        }
    }
}

fn index(op: ProgressOperation) -> usize {
    match op {
        ProgressOperation::Fill => 0,
        ProgressOperation::Erase => 1,
        ProgressOperation::Program => 2,
        ProgressOperation::Verify => 3,
    }
}

fn running(op: ProgressOperation) -> &'static str {
    match op {
        ProgressOperation::Fill => "Reading",
        ProgressOperation::Erase => "Erasing",
        ProgressOperation::Program => "Programming",
        ProgressOperation::Verify => "Verifying",
    }
}

fn done(op: ProgressOperation) -> &'static str {
    match op {
        ProgressOperation::Fill => "Read back",
        ProgressOperation::Erase => "Erased",
        ProgressOperation::Program => "Programmed",
        ProgressOperation::Verify => "Verified",
    }
}
