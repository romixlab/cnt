//! `cnt!`, `cnt_if!`, `bkp_cnt!` and `bkp_cnt_if!`: call-site counters.

use crate::symbol::{CallSite, Field, Severity, Storage, Ty, counter_marker_attrs, counter_symbol};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token, parse2};

/// `name: ty ["unit"] [+= expr] [, severity] [, group] [,]`
struct CounterArgs {
    name: Ident,
    ty: Ident,
    unit: Option<LitStr>,
    /// Value to add, 1 if omitted.
    rhs: Option<Expr>,
    severity: Option<Ident>,
    group: Option<Ident>,
}

/// `condition, <CounterArgs>`
struct CounterIfArgs {
    condition: Expr,
    counter: CounterArgs,
}

impl Parse for CounterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty = input.parse()?;

        let unit = if input.peek(LitStr) {
            Some(input.parse()?)
        } else {
            None
        };

        let rhs = if input.peek(Token![+=]) {
            input.parse::<Token![+=]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        let severity = parse_optional_ident(input)?;
        let group = parse_optional_ident(input)?;
        // Trailing comma
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error(
                "unexpected token, expected `+= <expr>`, `, <severity>`, `, <group>` or end of input",
            ));
        }
        Ok(Self {
            name,
            ty,
            unit,
            rhs,
            severity,
            group,
        })
    }
}

impl Parse for CounterIfArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let condition = input.parse()?;
        input.parse::<Token![,]>()?;
        let counter = input.parse()?;
        Ok(Self { condition, counter })
    }
}

fn parse_optional_ident(input: ParseStream) -> syn::Result<Option<Ident>> {
    if input.peek(Token![,]) && input.peek2(Ident) {
        input.parse::<Token![,]>()?;
        Ok(Some(input.parse()?))
    } else {
        Ok(None)
    }
}

pub(crate) fn cnt(args: TokenStream, storage: Storage) -> syn::Result<TokenStream> {
    let args = parse2::<CounterArgs>(args)?;
    increment(&args, storage)
}

pub(crate) fn cnt_if(args: TokenStream, storage: Storage) -> syn::Result<TokenStream> {
    let args = parse2::<CounterIfArgs>(args)?;
    let condition = &args.condition;
    let increment = increment(&args.counter, storage)?;
    Ok(quote! {
        if #condition {
            #increment
        }
    })
}

/// `::cnt::saturating_add_<ty>_<storage>(<index>, <rhs>);`
///
/// No locals are introduced, so that `rhs` can't accidentally refer to them.
fn increment(args: &CounterArgs, storage: Storage) -> syn::Result<TokenStream> {
    let ty = Ty::parse(&args.ty.to_string()).ok_or_else(|| {
        syn::Error::new(
            args.ty.span(),
            "only `u32` and `u64` counters are supported.",
        )
    })?;
    let severity = match &args.severity {
        Some(ident) => Severity::parse(&ident.to_string()).ok_or_else(|| {
            syn::Error::new(
                ident.span(),
                format!(
                    "unknown severity: {ident}, supported: {}",
                    Severity::SUPPORTED
                ),
            )
        })?,
        None => Severity::Info,
    };
    let name = args.name.to_string();
    let unit = args.unit.as_ref().map(|u| u.value()).unwrap_or_default();
    let group = args
        .group
        .as_ref()
        .map(|g| g.to_string())
        .unwrap_or_default();
    let field = Field {
        name: &name,
        ty,
        unit: &unit,
        severity,
    };
    let symbol = counter_symbol(&field, &group, &CallSite::here());
    let attrs = counter_marker_attrs(storage, &symbol);
    let words = ty.words();
    let storage_tokens = storage.tokens();

    let increment_fn = Ident::new(
        &format!("saturating_add_{}_{}", ty.as_str(), storage.as_str()),
        Span::call_site(),
    );
    let rhs = match &args.rhs {
        Some(rhs) => quote!(#rhs),
        None => quote!(1),
    };
    if cfg!(feature = "disabled") {
        // Type-check `rhs` without evaluating it, so that the disabled build cannot rot
        let rhs_ty = match ty {
            Ty::U32 => quote!(u32),
            Ty::U64 => quote!(u64),
        };
        return Ok(quote! {
            if false {
                let _: #rhs_ty = #rhs;
            }
        });
    }
    Ok(quote! {
        ::cnt::#increment_fn(
            {
                // `#[used]` keeps the counter in the ELF even if the increment is optimized out
                #attrs
                static CNT_MARKER: [u8; #words] = [0; #words];
                static CNT_INDEX: ::cnt::Index = ::cnt::Index::new();
                CNT_INDEX.get(CNT_MARKER.as_ptr(), #words, ::cnt::Storage::#storage_tokens)
            },
            #rhs,
        );
    })
}
