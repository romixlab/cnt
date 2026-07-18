use syn::{Expr, Ident, LitStr, Token, parse::Parse};

pub struct ExprAndNameArgs {
    pub condition: Expr,
    pub name: Ident,
    pub ty: Ident,
    pub unit: Option<LitStr>,
    pub rhs: Expr,
    pub severity: Option<Ident>,
    pub group: Option<Ident>,
}

impl Parse for ExprAndNameArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let condition = input.parse()?;
        let _comma = input.parse::<Token![,]>()?;
        let name = input.parse()?;
        let _colon = input.parse::<Token![:]>()?;
        let ty = input.parse()?;

        let fork = input.fork();
        let unit = if fork.parse::<LitStr>().is_ok() {
            Some(input.parse()?)
        } else {
            None
        };

        let _inc = input.parse::<Token![+=]>()?;
        let rhs = input.parse()?;
        let severity = parse_optional_ident(input)?;
        let group = parse_optional_ident(input)?;
        Ok(Self {
            condition,
            name,
            ty,
            unit,
            rhs,
            severity,
            group,
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
