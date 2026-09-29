use proc_macro::TokenStream;

mod cnt_if;
mod construct;
mod input_args;
mod symbol;

/// Increment RAM counter if expression evaluates to true. RAM counters are reset to zero on firmware restart (by startup code).
///
/// Counters buffer size is controlled through the `CNT_RAM_BUFFER_SIZE_WORDS` env variable.
/// cnt cli tool checks that the actual number of counters used does not overflow the buffer, if you don't use
/// the tool, then you can check manually, for example, by inspecting cargo nm output.
///
/// Examples are `no_run`: counter indices come from the `cnt.x` linker script, so they are only valid in firmware.
///
/// Create and increment a counter named `blink_count` every time this line is executed:
/// ```no_run
/// # use cnt::cnt_if;
/// cnt_if!(true, blink_count: u32 += 1); // increment unconditionally
/// ```
///
/// Increment only when there is an error:
/// ```no_run
/// # use cnt::cnt_if;
/// let r: Result<(), ()> = Err(()); // some process returning a Result
/// cnt_if!(r.is_err(), err_count: u32 += 1);
/// ```
///
/// Any expression is supported instead of one:
/// ```no_run
/// # use cnt::cnt_if;
/// # let request = [0u8; 4];
/// cnt_if!(true, event_count: u32 += 1 + request.len() as u32);
/// ```
///
/// `u64` is supported as well:
/// ```no_run
/// # use cnt::cnt_if;
/// cnt_if!(true, uptime: u64 += 1); // consumes 2 words
/// ```
///
/// Set severity, default is info, supported: error, warn, info, debug, trace:
/// ```no_run
/// # use cnt::cnt_if;
/// cnt_if!(true, bytes_lost: u64 += 1, warn);
/// ```
///
/// Set group name, useful when there are many counters in use:
/// ```no_run
/// # use cnt::cnt_if;
/// # let buf = [0u8; 4];
/// cnt_if!(true, bytes_rx: u64 += buf.len() as u64, debug, usart);
/// ```
///
/// Set unit for better readability, upstream software can then convert from e.g., Bytes to KiB or MiB automatically:
/// ```no_run
/// # use cnt::cnt_if;
/// # let buf = [0u8; 4];
/// cnt_if!(true, bytes_rx: u64 "B" += buf.len() as u64, debug, usart);
/// ```
#[proc_macro]
pub fn cnt_if(args: TokenStream) -> TokenStream {
    match cnt_if::cnt_if(args.into()) {
        Ok(result) => result.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

/// Increment non-volatile counter if expression evaluates to true. Non-volatile counters buffer is supposed to be placed into
/// BKPRAM memory, or into RCC or TAMP registers, that do not lose contents on reset (provided there is a battery connected).
///
/// Counters buffer size is controlled through CNT_BKP_BUFFER_SIZE_WORDS env variable (default is 0).
/// bedrock cli tool checks that actual number of counters used to dot overflow the buffer, if you don't use
/// the tool, then you can check manually, for example, by inspecting cargo nm output.
#[proc_macro]
pub fn bkp_cnt_if(args: TokenStream) -> TokenStream {
    match cnt_if::bkp_cnt_if(args.into()) {
        Ok(result) => result.into(),
        Err(e) => e.into_compile_error().into(),
    }
}
