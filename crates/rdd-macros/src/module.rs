//! `module!` (§6.2): la expresión que construye un `Module`.
//!
//! Las dos formas del spec se expanden al mismo constructor:
//!
//! ```text
//! module! { providers: [TaskService], controllers: [TaskController], middlewares: [] }
//! module!(service::TaskService, routes::TaskController)
//! ```
//!
//! La forma abreviada no puede clasificar tipos dentro de la macro —Rust no tiene
//! reflexión—, así que delega en `ModuleMember`, que `#[service]` y
//! `#[controller]` implementan cada uno a su manera.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, Ident, Type, bracketed};

/// Contenido de la macro: con claves o como lista de miembros.
enum Form {
    Keyed {
        providers: Vec<Type>,
        controllers: Vec<Type>,
        middlewares: Vec<Expr>,
    },
    Members(Vec<Type>),
}

pub(crate) fn expand(args: TokenStream) -> TokenStream {
    let inner = unwrap_group(args);
    match syn::parse2::<Form>(inner) {
        Ok(form) => render(form),
        Err(err) => err.to_compile_error(),
    }
}

/// `module! { … }` y `module!(…)` llegan con el delimitador como único token.
fn unwrap_group(args: TokenStream) -> TokenStream {
    let mut iter = args.clone().into_iter();
    match (iter.next(), iter.next()) {
        (Some(proc_macro2::TokenTree::Group(group)), None) => group.stream(),
        _ => args,
    }
}

impl Parse for Form {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if is_keyed(input) {
            let mut providers = Vec::new();
            let mut controllers = Vec::new();
            let mut middlewares = Vec::new();

            while !input.is_empty() {
                let key: Ident = input.parse()?;
                input.parse::<syn::Token![:]>()?;
                let content;
                let _ = bracketed!(content in input);
                match key.to_string().as_str() {
                    "providers" => providers = types(&content)?,
                    "controllers" => controllers = types(&content)?,
                    "middlewares" => middlewares = exprs(&content)?,
                    other => {
                        return Err(syn::Error::new_spanned(
                            key,
                            format!(
                                "clave desconocida `{other}` en `module!`; las admitidas son \
                                 `providers`, `controllers` y `middlewares` (§6.2)"
                            ),
                        ));
                    }
                }
                if !input.is_empty() {
                    input.parse::<syn::Token![,]>()?;
                }
            }
            return Ok(Self::Keyed {
                providers,
                controllers,
                middlewares,
            });
        }

        Ok(Self::Members(types(input)?))
    }
}

fn types(input: ParseStream) -> syn::Result<Vec<Type>> {
    let parsed = Punctuated::<Type, syn::Token![,]>::parse_terminated(input)?;
    Ok(parsed.into_iter().collect())
}

fn exprs(input: ParseStream) -> syn::Result<Vec<Expr>> {
    let parsed = Punctuated::<Expr, syn::Token![,]>::parse_terminated(input)?;
    Ok(parsed.into_iter().collect())
}

/// La forma con claves empieza por `ident :`, no por un path `a::B`.
fn is_keyed(input: ParseStream) -> bool {
    let fork = input.fork();
    fork.parse::<Ident>().is_ok() && fork.peek(syn::Token![:]) && !fork.peek(syn::Token![::])
}

fn render(form: Form) -> TokenStream {
    let base = quote! { ::rdd::ModuleBuilder::named(::core::module_path!()) };
    match form {
        Form::Keyed {
            providers,
            controllers,
            middlewares,
        } => {
            let provide = providers.iter().map(|ty| {
                quote_spanned! { ty.span() =>
                    .provider(<#ty as ::rdd::ProviderMarker>::reg())
                }
            });
            let control = controllers
                .iter()
                .map(|ty| quote_spanned! { ty.span() => .controller::<#ty>() });
            let mws = middlewares
                .iter()
                .map(|expr| quote_spanned! { expr.span() => .middleware(#expr) });
            quote! { #base #(#provide)* #(#control)* #(#mws)* .build() }
        }
        Form::Members(members) => {
            let member = members
                .iter()
                .map(|ty| quote_spanned! { ty.span() => .member::<#ty>() });
            quote! { #base #(#member)* .build() }
        }
    }
}
