use proc_macro::TokenStream;

mod count;
mod counter;
mod symbol;

use symbol::Storage;

fn expand(result: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    match result {
        Ok(result) => result.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

/// Increment a RAM counter. RAM counters are reset to zero on firmware restart (by startup code).
///
/// Counters buffer size is controlled through the `CNT_RAM_BUFFER_SIZE_WORDS` env variable. The `cnt.x` linker
/// script fails the link if there are more counters than the buffer can hold.
///
/// The generated code refers to the `cnt` crate by that name, so it must not be renamed in `Cargo.toml`.
///
/// Examples are `no_run`: counter indices come from the `cnt.x` linker script, so they are only valid in firmware.
///
/// Create and increment a counter named `blink_count` every time this line is executed:
/// ```no_run
/// # use cnt::cnt;
/// cnt!(blink_count: u32); // `+= 1` is implied
/// ```
///
/// Any expression is supported instead of one:
/// ```no_run
/// # use cnt::cnt;
/// # let request = [0u8; 4];
/// cnt!(event_count: u32 += 1 + request.len() as u32);
/// ```
///
/// `u64` is supported as well:
/// ```no_run
/// # use cnt::cnt;
/// cnt!(uptime: u64); // consumes 2 words
/// ```
///
/// Set severity, default is info, supported: error, warn, info, debug, trace:
/// ```no_run
/// # use cnt::cnt;
/// cnt!(bytes_lost: u64, warn);
/// ```
///
/// Set group name, useful when there are many counters in use:
/// ```no_run
/// # use cnt::cnt;
/// # let buf = [0u8; 4];
/// cnt!(bytes_rx: u64 += buf.len() as u64, debug, usart);
/// ```
///
/// Set unit for better readability, upstream software can then convert from e.g., Bytes to KiB or MiB automatically:
/// ```no_run
/// # use cnt::cnt;
/// # let buf = [0u8; 4];
/// cnt!(bytes_rx: u64 "B" += buf.len() as u64, debug, usart);
/// ```
#[proc_macro]
pub fn cnt(args: TokenStream) -> TokenStream {
    expand(counter::cnt(args.into(), Storage::Ram))
}

/// Increment a RAM counter if the expression evaluates to true. Takes the condition followed by the same arguments
/// as [`cnt!`].
///
/// ```no_run
/// # use cnt::cnt_if;
/// let r: Result<(), ()> = Err(()); // some process returning a Result
/// cnt_if!(r.is_err(), err_count: u32);
/// ```
#[proc_macro]
pub fn cnt_if(args: TokenStream) -> TokenStream {
    expand(counter::cnt_if(args.into(), Storage::Ram))
}

/// Increment a non-volatile counter. Non-volatile counters buffer is supposed to be placed into BKPRAM memory, or
/// into RCC or TAMP registers, that do not lose contents on reset (provided there is a battery connected).
///
/// Counters buffer size is controlled through the `CNT_BKP_BUFFER_SIZE_WORDS` env variable (default is 0), the memory
/// region the buffer is placed in through `CNT_BKP_MEMORY_REGION` (default: `BKPSRAM`). The `cnt.x` linker script
/// fails the link if there are more counters than the buffer can hold.
///
/// Arguments are the same as for [`cnt!`].
#[proc_macro]
pub fn bkp_cnt(args: TokenStream) -> TokenStream {
    expand(counter::cnt(args.into(), Storage::Bkp))
}

/// Increment a non-volatile counter if the expression evaluates to true, see [`bkp_cnt!`] and [`cnt_if!`].
#[proc_macro]
pub fn bkp_cnt_if(args: TokenStream) -> TokenStream {
    expand(counter::cnt_if(args.into(), Storage::Bkp))
}

/// Derive `cnt::Count` for an enum with unit variants, each variant becomes a counter.
///
/// Variants may be annotated with `#[count(...)]` taking, in any order: `u32` or `u64` (default `u32`), a severity
/// (`error`, `warn`, `info`, `debug`, `trace`, default `info`) and `unit = "..."`.
///
/// ```no_run
/// #[derive(cnt::Count)]
/// enum FramerEvent {
///     CrcError,
///     #[count(warn)]
///     Overrun,
///     #[count(u64, unit = "B", debug)]
///     RxBytes,
/// }
/// ```
///
/// Instances of the counters are created with [`counters!`] or [`bkp_counters!`].
#[proc_macro_derive(Count, attributes(count))]
pub fn derive_count(input: TokenStream) -> TokenStream {
    expand(count::derive_count(input.into()))
}

/// Create a `cnt::Counters<E>` in the RAM buffer, for use in a `static`. Takes the `Count` type and an instance name
/// shown by host tools, e.g. `counters!(FramerEvent, uart1)`.
///
/// Each invocation reserves its own words in the buffer, so a library taking a `&'static Counters<E>` gets separate
/// counters per instance:
///
/// ```no_run
/// # #[derive(cnt::Count)] enum FramerEvent { CrcError }
/// static UART1_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::counters!(FramerEvent, uart1);
/// static UART2_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::counters!(FramerEvent, uart2);
/// UART1_FRAMER_CNT.count(FramerEvent::CrcError);
/// ```
#[proc_macro]
pub fn counters(args: TokenStream) -> TokenStream {
    expand(count::counters(args.into(), Storage::Ram))
}

/// Create a `cnt::Counters<E>` in the non-volatile buffer, see [`counters!`] and [`bkp_cnt!`].
#[proc_macro]
pub fn bkp_counters(args: TokenStream) -> TokenStream {
    expand(count::counters(args.into(), Storage::Bkp))
}
