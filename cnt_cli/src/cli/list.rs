use crate::theme::theme;
use anstream::{print, println};
use cnt_core::{Counter, Counters, CountersBlock, Severity};

/// Buffer usage above this percentage produces a warning.
const USAGE_WARN_PERCENT: u64 = 95;

pub fn list(counters: &Counters) {
    let hint = theme().hint;
    if let Some(ram) = counters.ram_counters() {
        list_entries(ram);
        if counters.bkp_counters().is_some() {
            println!();
        }
    } else {
        println!("{hint}No RAM counters found{hint:#}");
    }
    if let Some(bkp) = counters.bkp_counters() {
        list_entries(bkp);
    } else {
        println!("{hint}No BKP counters found{hint:#}");
    }
}

fn list_entries(cnt: &CountersBlock) {
    let t = theme();
    print_header(cnt);
    let width = name_width(cnt);
    let mut total_size = 0;
    for c in cnt.entries().values() {
        let hint = t.hint;
        print!(
            "{hint}{}{hint:#} {:width$}",
            severity_label(c.severity),
            c.name
        );
        print!(" {hint}{}{hint:#}", c.ty);
        if !c.unit.is_empty() {
            print!(" {hint}`{}`{hint:#}", c.unit);
        }
        println!();
        print_location(c);
        total_size += c.ty.len() as u64;
    }
    print_usage(cnt, total_size);
}

/// `📍 RAM counters` followed by the buffer address and size.
pub(super) fn print_header(cnt: &CountersBlock) {
    let t = theme();
    let (header, hint) = (t.header, t.hint);
    println!(
        "📍 {header}{} counters{header:#} {hint}at 0x{:08x}, {} B{hint:#}",
        cnt.storage(),
        cnt.buffer().addr,
        cnt.buffer().size
    );
}

/// Width of the longest counter name in a block, for aligning columns.
pub(super) fn name_width(cnt: &CountersBlock) -> usize {
    cnt.entries()
        .values()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(0)
}

/// Fixed-width severity label, as printed by defmt.
pub(super) fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warn => "WARN ",
        Severity::Info => "INFO ",
        Severity::Debug => "DEBUG",
        Severity::Trace => "TRACE",
    }
}

/// defmt-style location line: `└─ module @ file:line`.
pub(super) fn print_location(c: &Counter) {
    let t = theme();
    let (hint, path) = (t.hint, t.path);
    let Some(location) = &c.location else {
        println!("{hint}└─ <unknown location>{hint:#}");
        return;
    };
    print!("{hint}└─ ");
    if !location.module.is_empty() {
        print!("{} @ ", location.module);
    }
    println!(
        "{hint:#}{path}{}:{}{path:#}",
        location.file.display(),
        location.line
    );
}

/// Total and free space in the buffer, with a warning when it is (nearly) full.
fn print_usage(cnt: &CountersBlock, used: u64) {
    let t = theme();
    let hint = t.hint;
    let size = cnt.buffer().size;
    let free = size as i64 - used as i64;
    // Round up, so that a nearly full buffer is not reported as 100%
    let percent = if size == 0 {
        0
    } else {
        (used * 100).div_ceil(size)
    };
    println!(
        "{hint}{} used: {used} of {size} B ({percent}%), free: {free} B ({}x u32, {}x u64){hint:#}",
        cnt.storage(),
        free.max(0) / 4,
        free.max(0) / 8
    );
    if used > size {
        let error = t.error;
        println!(
            "🛑 {error}Not enough space for {} counters: {used} B needed, {size} B available{error:#}",
            cnt.storage()
        );
    } else if percent > USAGE_WARN_PERCENT {
        let warn = t.warn;
        println!(
            "⚠️ {warn}{} counters buffer is {percent}% full{warn:#}",
            cnt.storage()
        );
    }
}
