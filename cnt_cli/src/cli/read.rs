use cnt_core::{Counters, CountersBlock};
use probe_rs::probe::list::Lister;
use probe_rs::{Core, MemoryInterface, Permissions};

pub fn read(counters: &mut Counters) -> anyhow::Result<()> {
    if counters.is_empty() {
        println!("No counters found");
        return Ok(());
    }

    let lister = Lister::new();
    let probes = lister.list_all();
    let probe = probes[0].open()?;
    let mut session = probe.attach("STM32H533RE", Permissions::default())?;
    let mut core = session.core(0)?;

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
