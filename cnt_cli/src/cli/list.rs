use cnt_core::{Counters, CountersBlock};

pub fn list(counters: &Counters) {
    if let Some(ram) = counters.ram_counters() {
        list_entries(ram);
        if counters.bkp_counters().is_some() {
            println!();
        }
    } else {
        println!("No RAM counters found");
    }
    if let Some(bkp) = counters.bkp_counters() {
        list_entries(bkp);
    } else {
        println!("No BKP counters found");
    }
}

fn list_entries(cnt: &CountersBlock) {
    println!(
        "📍 {} address: 0x{:08x} len: {}B",
        cnt.storage(),
        cnt.buffer().addr,
        cnt.buffer().size
    );
    let mut total_size = 0;
    for c in cnt.entries().values() {
        println!("{}:{:?} {:?}", c.name, c.ty, c.location);
        total_size += c.ty.len();
    }
    println!("{} total size: {}B", cnt.storage(), total_size);
    let free = cnt.buffer().size as i64 - total_size as i64;
    println!(
        "{} free space: {}B ({}x u32, {}x u64)",
        cnt.storage(),
        free,
        free / 4,
        free / 8
    );
    if free < 0 {
        println!("🛑 Not enough space for {} counters!", cnt.storage());
    }
}
