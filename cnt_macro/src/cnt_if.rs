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
    let ty = match input.ty.to_string().as_str() {
        "u32" => Ty::U32,
        "u64" => Ty::U64,
        _ => {
            return Err(syn::Error::new(
                input.ty.span(),
                "only `u32` and `u64` counters are supported.",
            ));
        }
    };
    let ram_or_bkp = match storage {
        Storage::Ram => "ram",
        Storage::Bkp => "bkp",
    };
    let increment_fn = Ident::new(
        &format!("saturating_add_{}_{ram_or_bkp}", ty.as_str()),
        Span::call_site(),
    );
    let group = input.group.map(|g| g.to_string()).unwrap_or_default();
    let name = input.name.to_string();
    let unit = input.unit.map(|u| u.value()).unwrap_or_default();
    let severity = match &input.severity {
        Some(severity) => match severity.to_string().as_str() {
            "error" => Severity::Error,
            "warn" => Severity::Warn,
            "info" => Severity::Info,
            "debug" => Severity::Debug,
            "trace" => Severity::Trace,
            o => {
                return Err(syn::Error::new(
                    severity.span(),
                    format!("unknown severity: {o}, supported: error, warn, info, debug, trace"),
                ));
            }
        },
        None => Severity::Info,
    };
    let rhs = match input.rhs {
        Some(rhs) => quote!(#rhs),
        None => quote!(1),
    };
    let counter_idx = static_variable(&group, storage, &name, ty, &unit, severity);

    // No locals are introduced, so that `#expr` and `#rhs` can't accidentally refer to them.
    Ok(quote! {
        if #expr {
            ::cnt::#increment_fn(#counter_idx, #rhs);
        }
    })
}
