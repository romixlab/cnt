use syn::{Expr, Ident, Token, parse::Parse};

pub struct ExprAndNameArgs {
    pub expr: Expr,
    pub name: Ident,
    pub ty: Ident,
    pub severity: Option<Ident>,
    pub group: Option<Ident>,
}

impl Parse for ExprAndNameArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let expr = input.parse()?;
        let _comma = input.parse::<Token![,]>()?;
        let name = input.parse()?;
        let _colon = input.parse::<Token![:]>()?;
        let ty = input.parse()?;
        let severity = parse_optional_ident(input)?;
        let group = parse_optional_ident(input)?;
        Ok(Self {
            expr,
            name,
            ty,
            severity,
            group
        })
    }
}

fn parse_optional_ident(input: syn::parse::ParseStream) -> syn::Result<Option<Ident>> {
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
        Ok(Some(input.parse()?))
    } else {
        Ok(None)
    }
}