use syn::{Expr, Ident, LitStr, Token, parse::Parse};

pub struct ExprAndNameArgs {
    pub condition: Expr,
    pub name: Ident,
    pub ty: Ident,
    pub unit: Option<LitStr>,
    /// Value to add, 1 if omitted.
    pub rhs: Option<Expr>,
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
            return Err(input.error("unexpected token, expected `+= <expr>`, `, <severity>`, `, <group>` or end of input"));
        }
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
    if input.peek(Token![,]) && input.peek2(Ident) {
        input.parse::<Token![,]>()?;
        Ok(Some(input.parse()?))
    } else {
        Ok(None)
    }
}
