use syn::{Expr, Ident, Token, parse::Parse};

pub struct ExprAndNameArgs {
    pub expr: Expr,
    pub _comma: Token![,],
    pub name: Ident,
    pub _colon: Token![:],
    pub ty: Ident,
    // pub _comma2: Token![,],
    // pub expected: Option<Expr>,
}

impl Parse for ExprAndNameArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            expr: input.parse()?,
            _comma: input.parse()?,
            name: input.parse()?,
            _colon: input.parse()?,
            ty: input.parse()?,
            // _comma2: input.parse()?,
            // expected: input.parse()?,
        })
    }
}
