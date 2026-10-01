//! `cnt run`: flash the firmware and restart the target, like `probe-rs run`.

use super::logs::DefmtReader;
use crate::theme::theme;
use anstream::eprintln;
use anyhow::Context;
use probe_rs::Session;
use probe_rs::flashing::{
    DownloadOptions, ElfLoader, FlashProgress, ProgressEvent, ProgressOperation, build_loader,
};
use probe_rs::rtt::{Rtt, ScanRegion};
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

    let mut started = Instant::now();
    let mut options = DownloadOptions::new();
    options.progress = FlashProgress::new(|event| match event {
        ProgressEvent::Started(_) => started = Instant::now(),
        ProgressEvent::Finished(op) => eprintln!(
            "{hint}  {} in {:.2}s{hint:#}",
            done(op),
            started.elapsed().as_secs_f64()
        ),
        _ => {}
    });
    loader
        .commit(session, options)
        .context("Failed to flash the firmware")?;

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

fn done(op: ProgressOperation) -> &'static str {
    match op {
        ProgressOperation::Fill => "Read back",
        ProgressOperation::Erase => "Erased",
        ProgressOperation::Program => "Programmed",
        ProgressOperation::Verify => "Verified",
    }
}
