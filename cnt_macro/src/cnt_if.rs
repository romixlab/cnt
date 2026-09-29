use crate::construct::static_variable;
use crate::input_args::ExprAndNameArgs;
use crate::symbol::{Severity, Storage, Ty};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::parse2;

pub(crate) fn cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, Storage::Ram)
}

pub(crate) fn bkp_cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, Storage::Bkp)
}

fn inner(args: TokenStream, storage: Storage) -> syn::Result<TokenStream> {
    let input = parse2::<ExprAndNameArgs>(args)?;
    let expr = &input.condition;
    if input.ty != "u32" && input.ty != "u64" {
        return Err(syn::Error::new(
            input.ty.span(),
            "only `u32` and `u64` counters are supported.",
        ));
    }
    let ram_or_bkp = match storage {
        Storage::Ram => "ram",
        Storage::Bkp => "bkp",
    };
    let increment_fn = Ident::new(
        format!("saturating_add_{}_{ram_or_bkp}", input.ty).as_str(),
        Span::call_site(),
    );
    let group = input.group.map(|g| g.to_string()).unwrap_or_default();
    let name = input.name.to_string();
    let unit = input.unit.map(|u| u.value()).unwrap_or_default();
    let severity = if let Some(severity) = input.severity {
        match severity.to_string().as_str() {
            "error" => Severity::Error,
            "warn" => Severity::Warn,
            "info" => Severity::Info,
            "debug" => Severity::Debug,
            "trace" => Severity::Trace,
            o => {
                return Err(syn::Error::new(
                    input.ty.span(),
                    format!("unknown severity: {o}, supported: error, warn, info, debug, trace"),
                ));
            }
        }
    } else {
        Severity::Info
    };
    let rhs = input.rhs;
    let tokens = match input.ty.to_string().as_str() {
        "u32" => {
            let counter_idx =
                static_variable(group.as_str(), storage, &name, Ty::U32, &unit, severity, 1);
            quote! {
                if #expr {
                    let counter_idx = #counter_idx;
                    unsafe { cnt::#increment_fn(counter_idx, #rhs); };
                }
            }
        }
        "u64" => {
            let counter_idx =
                static_variable(group.as_str(), storage, &name, Ty::U64, &unit, severity, 2);
            quote! {
                if #expr {
                    let counter_idx = #counter_idx;
                    unsafe { cnt::#increment_fn(counter_idx, #rhs); };
                }
            }
        }
        _ => unreachable!(),
    };

    // eprintln!("{tokens}");
    Ok(tokens)
}
