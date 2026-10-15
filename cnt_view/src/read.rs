//! `read` and `read --watch`: counter values, once or as they change, with the firmware's logs.

use crate::keys::{QuitKey, restore_terminal};
use crate::list::{name_width, print_header, print_location, severity_label};
use crate::output::{self, Format};
use crate::theme::theme;
use anstream::{eprintln, println};
use cnt_core::{Counter, Counters, CountersBlock, Event, Log, Source, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Print counters and their values, after [`Counters::read`].
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

/// How often logs are read in watch mode. RTT buffers are small, and the firmware blocks when they are full.
const LOG_INTERVAL: Duration = Duration::from_millis(10);

/// Read counters every `interval` and print the ones that changed, together with the source's logs, until
/// interrupted (Ctrl+C, or `q` when stdin is a terminal). The source is closed at the end, also after an error.
pub fn watch(
    counters: &mut Counters,
    source: &mut dyn Source,
    format: Format,
    interval: Duration,
) -> anyhow::Result<()> {
    let stop = stop_on_ctrl_c()?;
    let mut quit = QuitKey::new()?;
    if quit.is_some() && format == Format::Text {
        let hint = theme().hint;
        eprintln!("{hint}Press q or Ctrl+C to stop{hint:#}");
    }
    let result = watch_until(counters, source, format, interval, &stop, quit.as_mut());
    drop(quit);
    // Also after errors, as the firmware blocks on a full log buffer otherwise
    let closed = source.close();
    result.and(closed)
}

fn watch_until(
    counters: &mut Counters,
    source: &mut dyn Source,
    format: Format,
    interval: Duration,
    stop: &AtomicBool,
    mut quit: Option<&mut QuitKey>,
) -> anyhow::Result<()> {
    let start = Instant::now();
    let logs = source.logs_attached().is_some();
    let mut next_read = start;
    while !stop.load(Ordering::Relaxed) {
        if logs {
            for log in source.poll_logs()? {
                match format {
                    Format::Text => print_log(start.elapsed(), &log),
                    _ => output::log_event(unix_millis(), &log)?,
                }
            }
        }
        if Instant::now() >= next_read {
            // Fixed rate rather than fixed delay, so that slow reads do not make the interval drift
            let first = next_read == start;
            next_read += interval;
            if first && format == Format::Text {
                counters.read_changes(source)?;
                print(counters);
            } else {
                print_changes(counters, source, format, start)?;
            }
        }
        let wake = if logs {
            next_read.min(Instant::now() + LOG_INTERVAL)
        } else {
            next_read
        };
        let timeout = wake.saturating_duration_since(Instant::now());
        let quit_pressed = match quit.as_deref_mut() {
            Some(quit) => quit.wait(timeout)?,
            None => {
                std::thread::sleep(timeout);
                false
            }
        };
        if quit_pressed {
            break;
        }
    }
    Ok(())
}

/// Read counters and print the ones that changed since the previous read, all of them on the first.
fn print_changes(
    counters: &mut Counters,
    source: &mut dyn Source,
    format: Format,
    start: Instant,
) -> anyhow::Result<()> {
    let ts = unix_millis();
    for change in counters.read_changes(source)? {
        match format {
            Format::Text => {
                print_event(change.event, start.elapsed(), change.counter, change.value)
            }
            _ => output::watch_event(
                change.event,
                ts,
                change.storage,
                change.counter,
                change.value,
            )?,
        }
    }
    Ok(())
}

/// Ask the watch loop to stop on Ctrl+C, so that it can clean up. A second Ctrl+C exits right away.
fn stop_on_ctrl_c() -> anyhow::Result<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || {
        if flag.swap(true, Ordering::Relaxed) {
            restore_terminal();
            std::process::exit(130);
        }
    })?;
    Ok(stop)
}

/// `+1.234s INFO  0.000123 message` line of a defmt log, followed by its location.
fn print_log(elapsed: Duration, log: &Log) {
    let t = theme();
    let (hint, path) = (t.hint, t.path);
    let style = log.severity.map_or(t.table, |s| t.severity(s));
    let timestamp = log
        .timestamp
        .as_ref()
        .map(|ts| format!(" {hint}{ts}{hint:#}"))
        .unwrap_or_default();
    println!(
        "{hint}+{:.3}s{hint:#} {style}{}{style:#}{timestamp} {}",
        elapsed.as_secs_f64(),
        level_label(log),
        log.message
    );
    if let (Some(loc), Some(file_line)) = (&log.location, log.file_line()) {
        println!(
            "{hint}└─ {} @ {hint:#}{path}{file_line}{path:#}",
            loc.module
        );
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

/// Fixed-width level label of a log, blank for `println!`.
pub fn level_label(log: &Log) -> &'static str {
    log.severity.map_or("     ", severity_label)
}
