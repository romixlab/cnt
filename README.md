# cnt

![Crates.io Version](https://img.shields.io/crates/v/cnt)

<p align="center">
<img src="https://github.com/romixlab/cnt/blob/main/assets/logo.png?raw=true" alt="logo" width="256"/>
</p>

> When logging is not an option - count

In microcontroller firmwares it is not always possible or desirable to log things, for example due to:
* Timing constraints in interrupts
* Memory constraints
* Absence of a logging interface
* Absence of log recording from firmware boot, making accurate calculations impossible
* Inconvenience of analyzing log output

This crate provide a convenient way to count events, errors or anything else, using a continuous RAM array:

```rust
fn high_frequency_irq() {
    let r = do_something();
    cnt::cnt_if!(r.is_err(), unpack_errors: u64);
}
```

Under the hood, a simple linker trick is used to obtain a unique ID for each count statement (similar to defmt).
Then an element of a `_CNT_RAM_BUFFER` is increment (or two in the case of u64).

u32 counters are supported as well, and you can pass `true` to count unconditionally:

```rust
fn process_packet() {
    cnt::cnt_if!(true, packet_count: u32);
}
```

## How to use

* Add `cnt = "0.1.0"` to `Cargo.coml`
* Add `"-C", "link-arg=-Tcnt.x",` to `config.toml`
* Optionally set `CNT_RAM_BUFFER_SIZE_WORDS` in the `[env]` section as well, default value is 64 words (256 bytes).

## How to get counters data from fw itself

Call `counters_ram_buffer`:

```rust
fn main() {
    let counters: &[u32] = cnt::counters_ram_buffer();
    // send using whatever interface to host
}
```

## How to get counters data live from a running device

Basic idea is to read `_CNT_RAM_BUFFER` from RAM using JTAG or SWD interface. A CLI tool to do that is not yet ready though.

## How to get IDs

CLI tool to analyze ELF and present the data in a nice way is not yet ready. But for now, a simple nm command will
show all the counters present:

```shell
arm-none-eabi-nm ./path/to/elf_fw | grep cnt_ram
```

## Why there are no compile-time filters?

Based on the assumption, that there won't be that many counters, it was decided to not implement compile-time filters for simplicity at the moment (info/warn/error/trace). For example, 100 `u32` counters consume only 400 bytes of RAM.

Having filters would also require reprogramming a device in order to enable more levels, which might be undesirable.

