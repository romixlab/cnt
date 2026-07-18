use proc_macro::TokenStream;

mod cnt_if;
mod construct;
mod input_args;
mod symbol;

/// Increment RAM counter if expression evaluates to true. RAM counters are reset to zero on firmware restart (by startup code).
///
/// Counters buffer size is controlled through CNT_RAM_BUFFER_SIZE_WORDS env variable.
/// bedrock cli tool checks that actual number of counters used to dot overflow the buffer, if you don't use
/// the tool, then you can check manually, for example, by inspecting cargo nm output.
///
/// Example:
/// ```
/// use cnt_macro::cnt_if;
///
/// cnt_if!(true, blink_count: u32); // increment unconditionally
///
/// let r = Err(()); // some process returning a Result
/// cnt_if!(r.is_err(), err_count: u32);
///
/// cnt_if!(true, uptime: u64); // consumes 2 words
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
/// TODO: reset counters on firmware reflash through the cli tool.
///
/// Counters buffer size is controlled through CNT_BKP_BUFFER_SIZE_WORDS env variable (default is 0).
/// bedrock cli tool checks that actual number of counters used to dot overflow the buffer, if you don't use
/// the tool, then you can check manually, for example, by inspecting cargo nm output.
///
/// Example:
/// ```
/// use cnt_macro::bkp_cnt_if;
///
/// bkp_cnt_if!(true, hard_fault_count_total: u32); // increment unconditionally
///
/// let r = Err(()); // some process returning a Result
/// bkp_cnt_if!(r.is_err(), err_count_total: u32);
///
/// bkp_cnt_if!(true, impulses_total: u64); // consumes 2 words
/// ```
#[proc_macro]
pub fn bkp_cnt_if(args: TokenStream) -> TokenStream {
    match cnt_if::bkp_cnt_if(args.into()) {
        Ok(result) => result.into(),
        Err(e) => e.into_compile_error().into(),
    }
}
