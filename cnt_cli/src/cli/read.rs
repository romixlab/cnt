use super::list::{name_width, print_header, print_location, severity_label};
use super::output::{self, Event, Format};
use crate::theme::theme;
use anstream::println;
use cnt_core::{Counter, Counters, CountersBlock, Storage, Value};
use probe_rs::{Core, MemoryInterface};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Read the values of all counters from the target.
pub fn read(counters: &mut Counters, core: &mut Core) -> anyhow::Result<()> {
    for cnt in counters.blocks_mut() {
        let mut data = vec![0u8; cnt.buffer().size as usize];
        core.read_mem_32bit(cnt.buffer().addr, &mut data)?;
        cnt.read_values(&data)?;
    }
    Ok(())
}

/// Print counters and their values, after [`read`].
pub fn print(counters: &Counters) {
    if counters.is_empty() {
        let hint = theme().hint;
        println!("{hint}No counters found{hint:#}");
        return;
    }
    for (i, cnt) in counters.blocks().enumerate() {
        if i > 0 {
            println!();
        }
        print_counters(cnt);
    }
}

fn print_counters(counters: &CountersBlock) {
    let t = theme();
    print_header(counters);
    let width = name_width(counters);
    for (cnt, value) in counters.values() {
        // Only draw attention to counters that have fired
        let style = if value.to_u64() == 0 {
            t.hint
        } else {
            t.severity(cnt.severity)
        };
        let unit = if cnt.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", cnt.unit)
        };
        println!(
            "{style}{}{style:#} {:width$} {style}{value}{unit}{style:#}",
            severity_label(cnt.severity),
            cnt.qualified_name(),
        );
        print_location(cnt);
    }
}

/// Read counters every `interval` and print the ones that changed, until interrupted.
pub fn watch(
    counters: &mut Counters,
    core: &mut Core,
    format: Format,
    interval: Duration,
) -> anyhow::Result<()> {
    let start = Instant::now();
    read(counters, core)?;
    if format == Format::Text {
        print(counters);
    } else {
        let ts = unix_millis();
        for b in counters.blocks() {
            for (c, value) in b.values() {
                output::watch_event(Event::Initial, ts, b.storage(), c, value)?;
            }
        }
    }

    let mut next = start;
    loop {
        let previous: HashMap<(Storage, u64), u64> = counters
            .blocks()
            .flat_map(|b| b.values().map(|(c, v)| ((b.storage(), c.idx), v.to_u64())))
            .collect();
        // Fixed rate rather than fixed delay, so that slow reads do not make the interval drift
        next += interval;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        read(counters, core)?;

        let ts = unix_millis();
        for b in counters.blocks() {
            let changes: Vec<(&Counter, Value, u64)> = b
                .values()
                .filter_map(|(c, v)| {
                    let prev = *previous.get(&(b.storage(), c.idx))?;
                    (v.to_u64() != prev).then_some((c, v, prev))
                })
                .collect();
            let decreased = changes
                .iter()
                .filter(|(_, v, prev)| v.to_u64() < *prev)
                .count();
            // Resetting the target or the buffer zeroes all counters at once, anything else lowers single counters. All
            // counters that were non-zero going down at once is a reset, even if some have counted again since.
            let was_nonzero = previous
                .iter()
                .filter(|((storage, _), v)| *storage == b.storage() && **v != 0)
                .count();
            let is_reset = decreased > 0 && decreased == was_nonzero;
            for (c, value, prev) in changes {
                let event = if value.to_u64() > prev {
                    Event::Change
                } else if is_reset {
                    Event::Reset
                } else {
                    Event::Decrease
                };
                match format {
                    Format::Text => print_event(event, start.elapsed(), c, value),
                    _ => output::watch_event(event, ts, b.storage(), c, value)?,
                }
            }
        }
    }
}

/// `+1.234s ▲ name 42 unit` line of a changed counter in watch mode.
fn print_event(event: Event, elapsed: Duration, cnt: &Counter, value: Value) {
    let t = theme();
    let hint = t.hint;
    let style = t.severity(cnt.severity);
    let unit = if cnt.unit.is_empty() {
        String::new()
    } else {
        format!(" {}", cnt.unit)
    };
    let warn = t.warn;
    let marker = match event {
        Event::Reset => format!(" {warn}(reset){warn:#}"),
        Event::Decrease => format!(" {warn}(decreased){warn:#}"),
        Event::Initial | Event::Change => String::new(),
    };
    println!(
        "{hint}+{:.3}s{hint:#} {style}{}{style:#} {} {style}{value}{unit}{style:#}{marker}",
        elapsed.as_secs_f64(),
        severity_label(cnt.severity),
        cnt.qualified_name(),
    );
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
