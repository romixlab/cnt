use cnt_core::{Counters, CountersBlock};
use probe_rs::{Core, MemoryInterface};

pub fn read(counters: &mut Counters, mut core: Core) -> anyhow::Result<()> {
    if counters.is_empty() {
        println!("No counters found");
        return Ok(());
    }

    if let Some(cnt) = counters.ram_counters_mut() {
        read_counters(cnt, &mut core)?;
    }
    if let Some(cnt) = counters.bkp_counters_mut() {
        read_counters(cnt, &mut core)?;
    }

    Ok(())
}

fn read_counters(counters: &mut CountersBlock, core: &mut Core) -> anyhow::Result<()> {
    let mut data = vec![0u8; counters.buffer().size as usize];
    core.read_mem_32bit(counters.buffer().addr, &mut data)?;
    counters.read_values(&data)?;
    for (cnt, value) in counters.values() {
        println!("{}: {} {:?}", cnt.name, value, cnt.location);
    }
    Ok(())
}
