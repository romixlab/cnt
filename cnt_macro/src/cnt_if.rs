use crate::construct::{CounterKind, static_variable};
use crate::input_args::ExprAndNameArgs;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::parse2;

pub(crate) fn cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, CounterKind::RAM)
}

pub(crate) fn bkp_cnt_if(args: TokenStream) -> syn::Result<TokenStream> {
    inner(args, CounterKind::BKP)
}

fn inner(args: TokenStream, counter_kind: CounterKind) -> syn::Result<TokenStream> {
    let input = parse2::<ExprAndNameArgs>(args)?;
    let expr = &input.expr;
    let counter_name = &input.name;
    if input.ty != "u32" && input.ty != "u64" {
        return Err(syn::Error::new(
            input.ty.span(),
            "only `u32` and `u64` counters are supported.",
        ));
    }
    let ram_or_bkp = match counter_kind {
        CounterKind::RAM => "ram",
        CounterKind::BKP => "bkp",
    };
    let increment_fn = Ident::new(
        format!("increment_{}_{ram_or_bkp}", input.ty.to_string()).as_str(),
        Span::call_site(),
    );
    let tokens = match input.ty.to_string().as_str() {
        "u32" => {
            let data = format!("{counter_name}:{}", input.ty);
            let counter_idx = static_variable(counter_kind, data.as_str());
            quote! {
                if #expr {
                    let counter_idx = #counter_idx;
                    unsafe { cnt::#increment_fn(counter_idx); };
                }
            }
        }
        "u64" => {
            let data_lo = format!("{counter_name}:{},lo", input.ty);
            let counter_idx_lo = static_variable(counter_kind, data_lo.as_str());
            let data_hi = format!("{counter_name}:{},hi", input.ty);
            let counter_idx_hi = static_variable(counter_kind, data_hi.as_str());
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
