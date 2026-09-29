use super::list::{name_width, print_header, print_location, severity_label};
use super::theme::theme;
use anstream::println;
use cnt_core::{Counters, CountersBlock};
use probe_rs::{Core, MemoryInterface};

pub fn read(counters: &mut Counters, mut core: Core) -> anyhow::Result<()> {
    if counters.is_empty() {
        let hint = theme().hint;
        println!("{hint}No counters found{hint:#}");
        return Ok(());
    }

    let has_ram = counters.ram_counters().is_some();
    if let Some(cnt) = counters.ram_counters_mut() {
        read_counters(cnt, &mut core)?;
    }
    if let Some(cnt) = counters.bkp_counters_mut() {
        if has_ram {
            println!();
        }
        read_counters(cnt, &mut core)?;
    }

    Ok(())
}

fn read_counters(counters: &mut CountersBlock, core: &mut Core) -> anyhow::Result<()> {
    let mut data = vec![0u8; counters.buffer().size as usize];
    core.read_mem_32bit(counters.buffer().addr, &mut data)?;
    counters.read_values(&data)?;

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
            cnt.name,
        );
        print_location(cnt);
    }
    Ok(())
}
