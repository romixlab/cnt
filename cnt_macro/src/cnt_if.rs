use crate::construct::{static_variable};
use crate::input_args::ExprAndNameArgs;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::parse2;
use crate::symbol::{Severity, Storage, Ty};

pub(crate) fn cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, Storage::RAM)
}

pub(crate) fn bkp_cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, Storage::BKP)
}

fn inner(args: TokenStream, storage: Storage) -> syn::Result<TokenStream> {
    let input = parse2::<ExprAndNameArgs>(args)?;
    let expr = &input.expr;
    if input.ty != "u32" && input.ty != "u64" {
        return Err(syn::Error::new(
            input.ty.span(),
            "only `u32` and `u64` counters are supported.",
        ));
    }
    let ram_or_bkp = match storage {
        Storage::RAM => "ram",
        Storage::BKP => "bkp",
    };
    let increment_fn = Ident::new(
        format!("increment_{}_{ram_or_bkp}", input.ty.to_string()).as_str(),
        Span::call_site(),
    );
    let group = input.group.map(|g| g.to_string()).unwrap_or_default();
    let name = input.name.to_string();
    let severity = if let Some(severity) = input.severity {
        match severity.to_string().as_str() {
            "error" => Severity::Error,
            "warn" => Severity::Warn,
            "info" => Severity::Info,
            "debug" => Severity::Debug,
            "trace" => Severity::Trace,
            o => return Err(syn::Error::new(
                input.ty.span(),
                format!("unknown severity: {o}, supported: error, warn, info, debug, trace"),
            ))
        }
    } else {
        Severity::Info
    };
    let tokens = match input.ty.to_string().as_str() {
        "u32" => {
            let counter_idx = static_variable(group.as_str(), storage, &name, Ty::U32, severity);
            quote! {
                if #expr {
                    let counter_idx = #counter_idx;
                    unsafe { cnt::#increment_fn(counter_idx); };
                }
            }
        }
        "u64" => {
            let counter_idx_lo = static_variable(group.as_str(), storage, &name, Ty::U64Lo, severity);
            let counter_idx_hi = static_variable(group.as_str(), storage, &name, Ty::U64Hi, severity);
            quote! {
                if #expr {
                    let counter_idx_lo = #counter_idx_lo;
                    let counter_idx_hi = #counter_idx_hi;
                    unsafe { cnt::#increment_fn(counter_idx_lo, counter_idx_hi); };
                }
            }
        }
        _ => unreachable!(),
    };

    // eprintln!("{tokens}");
    Ok(tokens)
}
