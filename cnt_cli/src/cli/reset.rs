use cnt_core::CountersBlock;
use probe_rs::{Core, MemoryInterface};

pub fn reset(counters: &CountersBlock, core: &mut Core) -> anyhow::Result<()> {
    let data = vec![0u8; counters.buffer().size as usize];
    core.write_mem_32bit(counters.buffer().addr, &data)?;
    Ok(())
}
