//! `#[derive(Count)]` and `counters!`/`bkp_counters!`: instance counters.

use crate::symbol::{
    CallSite, Field, Severity, Storage, Ty, counter_marker_attrs, instance_symbols, layout_symbol,
    marker_attrs,
};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, LitStr, Meta, Token, Type, Variant, parse2};

/// Section of layout markers, placed at address 0 by `cnt.x`.
const LAYOUT_SECTION: &str = "cnt_layout";
/// Section of instance markers.
const INSTANCE_SECTION: &str = "cnt_instance";

/// Counter attributes of an enum variant: `#[count(u64, warn, unit = "B")]`, all optional and in any order.
struct VariantAttrs {
    ty: Ty,
    severity: Severity,
    unit: String,
}

impl Default for VariantAttrs {
    fn default() -> Self {
        Self {
            ty: Ty::U32,
            severity: Severity::Info,
            unit: String::new(),
        }
    }
}

impl Parse for VariantAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attrs = Self::default();
        let (mut has_ty, mut has_severity, mut has_unit) = (false, false, false);
        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let name = ident.to_string();
            if name == "unit" {
                input.parse::<Token![=]>()?;
                let unit: LitStr = input.parse()?;
                if has_unit {
                    return Err(syn::Error::new(ident.span(), "unit given twice"));
                }
                attrs.unit = unit.value();
                has_unit = true;
            } else if let Some(ty) = Ty::parse(&name) {
                if has_ty {
                    return Err(syn::Error::new(ident.span(), "type given twice"));
                }
                attrs.ty = ty;
                has_ty = true;
            } else if let Some(severity) = Severity::parse(&name) {
                if has_severity {
                    return Err(syn::Error::new(ident.span(), "severity given twice"));
                }
                attrs.severity = severity;
                has_severity = true;
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!(
                        "unknown counter attribute `{name}`, expected `u32`, `u64`, `unit = \"...\"` or a severity ({})",
                        Severity::SUPPORTED
                    ),
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(attrs)
    }
}

fn variant_attrs(variant: &Variant) -> syn::Result<VariantAttrs> {
    let mut found = None;
    for attr in &variant.attrs {
        if !attr.path().is_ident("count") {
            continue;
        }
        if found.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "only one `#[count(...)]` per variant",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(attr, "expected `#[count(...)]`"));
        };
        found = Some(parse2::<VariantAttrs>(list.tokens.clone())?);
    }
    Ok(found.unwrap_or_default())
}

pub(crate) fn derive_count(input: TokenStream) -> syn::Result<TokenStream> {
    let input = parse2::<DeriveInput>(input)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`Count` can only be derived for enums",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "`Count` cannot be derived for generic enums",
        ));
    }
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`Count` needs at least one variant",
        ));
    }

    let mut word = 0usize;
    let mut arms = vec![];
    let mut names = vec![];
    let mut attrs = vec![];
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.fields,
                "`Count` variants cannot have fields",
            ));
        }
        let a = variant_attrs(variant)?;
        let ident = &variant.ident;
        let ty = a.ty.tokens();
        arms.push(quote! {
            Self::#ident => ::cnt::Slot { word: #word, ty: ::cnt::Ty::#ty },
        });
        word += a.ty.words();
        names.push(ident.to_string());
        attrs.push(a);
    }
    let fields: Vec<Field> = names
        .iter()
        .zip(&attrs)
        .map(|(name, a)| Field {
            name,
            ty: a.ty,
            unit: &a.unit,
            severity: a.severity,
        })
        .collect();

    let ident = &input.ident;
    let words = word;
    let layout = if cfg!(feature = "disabled") {
        quote!(&0)
    } else {
        let symbol = layout_symbol(&ident.to_string(), &fields, &CallSite::here());
        let marker_attrs = marker_attrs(LAYOUT_SECTION, &symbol);
        quote! {{
            #marker_attrs
            static LAYOUT: u8 = 0;
            &LAYOUT
        }}
    };

    Ok(quote! {
        impl ::cnt::Count for #ident {
            const WORDS: usize = #words;
            const LAYOUT: &'static u8 = #layout;

            #[inline(always)]
            fn slot(&self) -> ::cnt::Slot {
                match self {
                    #(#arms)*
                }
            }
        }
    })
}

/// `Type, name [,]`
struct CountersArgs {
    ty: Type,
    name: Ident,
}

impl Parse for CountersArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ty = input.parse()?;
        input.parse::<Token![,]>()?;
        let name = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("unexpected token, expected `<Type>, <name>`"));
        }
        Ok(Self { ty, name })
    }
}

pub(crate) fn counters(args: TokenStream, storage: Storage) -> syn::Result<TokenStream> {
    let args = parse2::<CountersArgs>(args)?;
    let ty = &args.ty;
    if cfg!(feature = "disabled") {
        let storage = storage.tokens();
        return Ok(quote!(::cnt::Counters::<#ty>::disabled(::cnt::Storage::#storage)));
    }
    let (slots_symbol, info_symbol) =
        instance_symbols(&args.name.to_string(), storage, &CallSite::here());
    let slots_attrs = counter_marker_attrs(storage, &slots_symbol);
    let info_attrs = marker_attrs(INSTANCE_SECTION, &info_symbol);
    let storage = storage.tokens();

    Ok(quote! {
        {
            // One byte per word, its address in the counters section is the index of the first word
            #slots_attrs
            static SLOTS: [u8; <#ty as ::cnt::Count>::WORDS] = [0; <#ty as ::cnt::Count>::WORDS];
            // Ties the instance to its layout, both pointers are resolved by the linker in the ELF for host tools
            #info_attrs
            static INFO: ::cnt::InstanceInfo = ::cnt::InstanceInfo {
                slots: SLOTS.as_ptr(),
                layout: <#ty as ::cnt::Count>::LAYOUT as *const u8,
            };
            ::cnt::Counters::<#ty>::new(&SLOTS[0], ::cnt::Storage::#storage)
        }
    })
}
