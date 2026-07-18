use std::collections::BTreeMap;
use cnt_core::{Buffer, Counter, Counters};

pub fn list(counters: &Counters) {
    list_entries(counters.ram_counters(), counters.ram_buffer(), "RAM");
    println!();
    list_entries(counters.bkp_counters(), counters.bkp_buffer(), "BKP");
}

fn list_entries(counters: &BTreeMap<u64, Counter>, buffer: Option<Buffer>, storage: &'static str) {
    if let Some(buf) = buffer {
        println!("📍 {storage} address: 0x{:08x} len: {}B", buf.addr, buf.size);
    } else {
        println!("_CNT_{storage}_BUFFER not found");
    }
    let mut total_size = 0;
    for c in counters.values() {
        println!("{}:{:?} {:?}", c.name, c.ty, c.location);
        total_size += c.ty.len();
    }
    println!("{storage} total size: {}B", total_size);
    if let Some(buf) = buffer {
        let free = buf.size as i64 - total_size as i64;
        println!("{storage} free space: {}B ({}x u32, {}x u64)", free, free / 4, free / 8);
        if free < 0 {
            println!("🛑 Not enough space for {storage} counters!");
        }
    }
}