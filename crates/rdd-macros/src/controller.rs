//! Expansión de `#[controller]`, sus verbos y `#[rdd::main]` (§8.2, §8.4, §8.5).
//!
//! La forma generada sigue §8.5: una función `__rdd_route_<handler>()` por ruta
//! que devuelve `::rdd::http::RouteDef`, y un `routes()` público que las agrupa
//! (RF-056). Dos diferencias respecto al texto del spec, ambas forzadas por el
//! sistema de tipos y documentadas en el núcleo:
//!
//! - el extractor recibe un [`::rdd::http::Ctx`] **propietario** de la petición,
//!   no `(req, ctx)` prestados, porque los extractores devuelven datos con vida
//!   propia (ver `rdd::http::FromRequest`);
//! - el handler llega al núcleo como `Arc<dyn Fn…>` ya encajado por
//!   [`::rdd::api::route_def`], que es quien compone los middlewares de ruta y
//!   materializa los errores (RP-004, RP-005).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, FnArg, Ident, ImplItem, ItemFn, ItemImpl, LitInt, LitStr, Meta, Pat,
    ReturnType, Signature, Type,
};

/// Verbos admitidos dentro de `#[controller]` (§8.1).
const VERBS: [(&str, &str); 5] = [
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];

/// Punto de entrada de `#[controller(...)]`.
pub(crate) fn expand_controller(args: TokenStream, item: TokenStream) -> TokenStream {
    match controller_expansion(args, item) {
        Ok(tokens) => tokens,
        Err(errors) => combine(errors).to_compile_error(),
    }
}

/// Error de uso: `#[middleware]` suelto, fuera de un controller (§11.4).
pub(crate) fn standalone_middleware(args: TokenStream, item: TokenStream) -> TokenStream {
    error_at(
        if args.is_empty() {
            item.span()
        } else {
            args.span()
        },
        "RDD0401: `#[middleware(…)]` solo se interpreta dentro de un `impl` marcado con \
         `#[controller]` (§11.4)\n  help: el atributo acompaña a la declaración de una ruta; \
         sueltos, los middlewares se registran con `App::middleware(…)` o `Module::middleware(…)`",
    )
}

/// Error de uso: un verbo fuera de `#[controller]` (§8.2).
pub(crate) fn standalone_verb(verb: &str, attr: &str, item: TokenStream) -> TokenStream {
    error_at(
        item.span(),
        &format!(
            "RDD0301: `#[{attr}]` declara la ruta `{verb} …`, pero solo tiene sentido dentro de \
             un `impl` marcado con `#[controller]` (§8.2)\n  \
             help: mueve el método a un bloque `#[controller(\"/prefijo\")] impl MiController`"
        ),
    )
}

/// `#[rdd::main]`: runtime, `.env`, tracing y código de salida (§8.4).
pub(crate) fn expand_main(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return error_at(
            args.span(),
            "`#[rdd::main]` no recibe opciones; la configuración se hace en `App` (§8.4)",
        );
    }
    let fun = match syn::parse2::<ItemFn>(item) {
        Ok(fun) => fun,
        Err(err) => return err.to_compile_error(),
    };
    let mut errors = Vec::new();
    if fun.sig.asyncness.is_none() {
        errors.push(syn::Error::new_spanned(
            &fun.sig,
            "`#[rdd::main]` debe aplicarse a `async fn main()` (§8.4)",
        ));
    }
    if !fun.sig.inputs.is_empty() {
        errors.push(syn::Error::new_spanned(
            &fun.sig.inputs,
            "`main` no puede recibir argumentos",
        ));
    }
    let output = match &fun.sig.output {
        ReturnType::Default => {
            errors.push(syn::Error::new_spanned(
                &fun.sig.ident,
                "RDD0305: con `#[rdd::main]`, `main` debe devolver `rdd::Result<()>` para poder \
                 traducirse a código de salida (§8.4)",
            ));
            None
        }
        ReturnType::Type(_, ty) => Some(ty),
    };
    if !errors.is_empty() {
        return combine(errors).to_compile_error();
    }
    let attrs = &fun.attrs;
    let body = &fun.block;
    let ty = output.expect("validado arriba");

    quote! {
        #(#attrs)*
        async fn __rdd_main() -> #ty #body

        fn main() -> ::std::process::ExitCode {
            // §8.4: `.env`, `RUST_LOG`, runtime Tokio y código de salida.
            ::rdd::run_main(__rdd_main())
        }
    }
}

/// Argumentos de un atributo de verbo: `"/:id/complete"` y `status = 202` (§8.2).
struct VerbArgs {
    path: LitStr,
    status: Option<u16>,
    status_span: Span,
}

impl Parse for VerbArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        let mut status = None;
        let mut status_span = path.span();
        while !input.is_empty() {
            input.parse::<syn::Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            if key == "status" {
                let lit: LitInt = input.parse()?;
                status_span = lit.span();
                // RFC 9110 solo admite 100..=999; `StatusCode::from_u16` fallaría en runtime.
                let code = lit
                    .base10_parse::<u16>()
                    .ok()
                    .filter(|code| (100..=999).contains(code));
                let Some(code) = code else {
                    return Err(syn::Error::new_spanned(
                        &lit,
                        format!(
                            "RDD0303: `{}` no es un `status` válido; se quiere un decimal entre \
                             100 y 999 (RF-053)\n  help: `#[get(\"/tasks\", status = 201)]`",
                            lit.base10_digits()
                        ),
                    ));
                };
                status = Some(code);
            } else {
                return Err(syn::Error::new_spanned(
                    key,
                    "RDD0303: opción desconocida en el atributo de verbo; la única admitida es \
                     `status = NNN` (RF-053)",
                ));
            }
        }
        Ok(Self {
            path,
            status,
            status_span,
        })
    }
}

/// Lo que la macro necesita de cada handler declarado.
struct Route {
    /// Nombre del método del controller.
    handler: Ident,
    /// `::rdd::http::Method::GET` del verbo.
    method: TokenStream,
    /// Ruta final `prefijo + ruta`, normalizada (RF-051).
    path: String,
    /// Estado base de la ruta (RF-053).
    status: TokenStream,
    /// Expresiones de `#[middleware]`, controller primero y ruta después (§11.3).
    middlewares: Vec<Expr>,
    /// Llamadas a los extractores, en orden de firma (RF-057).
    extracts: Vec<TokenStream>,
    /// Invocación del handler con los valores ya extraídos.
    call: TokenStream,
    /// Span del atributo de verbo, para los diagnósticos.
    attr_span: Span,
}

fn controller_expansion(
    args: TokenStream,
    item: TokenStream,
) -> Result<TokenStream, Vec<syn::Error>> {
    let mut errors = Vec::new();

    let prefix = match parse_prefix(&args) {
        Ok(prefix) => prefix,
        Err(err) => {
            errors.push(err);
            String::new()
        }
    };

    let mut imp = match syn::parse2::<ItemImpl>(item) {
        Ok(imp) => imp,
        Err(err) => return Err(vec![err]),
    };
    if let Some((_, trait_path, _)) = &imp.trait_ {
        errors.push(syn::Error::new_spanned(
            trait_path,
            "#[controller] se aplica a un `impl` propio del tipo, no a la implementación de un \
             trait (§8.2)",
        ));
    }

    // Los `#[middleware]` del `impl` aplican a todas sus rutas (§11.2 D) y no
    // pueden quedarse en la salida: el compilador intentaría resolverlos.
    let controller_middlewares = take_middlewares(&mut imp.attrs, &mut errors);

    let self_ty = &imp.self_ty;
    let mut routes: Vec<Route> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();

    for item in &mut imp.items {
        let ImplItem::Fn(method) = item else { continue };
        let Some((verb, attr)) = take_verb(&mut method.attrs) else {
            errors.push(syn::Error::new_spanned(
                &method.sig.ident,
                "RDD0301: un método dentro de `#[controller]` debe llevar un atributo de verbo \
                 (`#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]`) o salir de este bloque \
                 (§8.2)",
            ));
            continue;
        };
        let attr_span = attr.span();
        let verb_args = match parse_verb_args(&attr) {
            Ok(parsed) => parsed,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        let path = match joined_path(&prefix, &verb_args.path.value(), verb_args.path.span()) {
            Ok(path) => path,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        if seen.iter().any(|(v, p)| *v == verb && *p == path) {
            errors.push(syn::Error::new_spanned(
                &attr,
                format!("RDD0304: ruta duplicada: `{verb} {path}` (§8.7)"),
            ));
            continue;
        }
        seen.push((verb.to_owned(), path.clone()));

        let mut middlewares = controller_middlewares.clone();
        middlewares.extend(take_middlewares(&mut method.attrs, &mut errors));

        match route_for(
            &method.sig,
            verb,
            &path,
            verb_args,
            middlewares,
            self_ty,
            attr_span,
        ) {
            Ok(route) => routes.push(route),
            Err(errs) => errors.extend(errs),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(render(imp, routes))
}

/// `prefix` de `#[controller("…")]`: vacío o empezando por `/` (RF-051, RDD0303).
fn parse_prefix(args: &TokenStream) -> syn::Result<String> {
    if args.is_empty() {
        return Ok(String::new());
    }
    let lit: LitStr = syn::parse2(args.clone()).map_err(|_| {
        syn::Error::new(
            args.span(),
            "RDD0303: `#[controller]` recibe el prefijo como cadena literal, p. ej. \
             `#[controller(\"/tasks\")]`",
        )
    })?;
    let value = lit.value();
    if !value.is_empty() && !value.starts_with('/') {
        return Err(syn::Error::new_spanned(
            &lit,
            "RDD0303: el prefijo de `#[controller]` debe empezar por `/` (§8.2)",
        ));
    }
    Ok(value)
}

/// Ruta final `prefijo + ruta` sin barras redundantes (RF-051).
fn joined_path(prefix: &str, route: &str, span: Span) -> syn::Result<String> {
    if !route.starts_with('/') {
        return Err(syn::Error::new(
            span,
            format!(
                "RDD0303: la ruta debe empezar por `/`, no `{route}` (§8.2)\n  \
                 help: `#[get(\"/\")]` para la raíz del prefijo"
            ),
        ));
    }
    let mut path = prefix.trim_end_matches('/').to_owned();
    let tail = route.trim_start_matches('/');
    if !tail.is_empty() {
        path.push('/');
        path.push_str(tail);
    }
    if path.is_empty() {
        path.push('/');
    }
    Ok(path)
}

/// Nombre del último segmento de la ruta, para los mensajes (`TaskController`).
fn type_label(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default(),
        other => quote!(#other).to_string(),
    }
}

fn is_verb(name: &str) -> bool {
    VERBS.iter().any(|(verb, _)| *verb == name)
}

fn verb_method(name: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(verb, _)| *verb == name)
        .map(|(_, method)| *method)
}

/// Nombre del último segmento de la ruta de un atributo (`middleware`, `get`…).
///
/// Sirve tanto para `#[get("…")]` como para `#[rdd::get("…")]` (§8.1).
fn attr_name(attr: &Attribute) -> Option<String> {
    attr.path()
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// Extrae el atributo de verbo del método, si lo tiene.
fn take_verb(attrs: &mut Vec<Attribute>) -> Option<(&'static str, Attribute)> {
    let index = attrs
        .iter()
        .position(|attr| attr_name(attr).is_some_and(|name| is_verb(&name)))?;
    let attr = attrs.remove(index);
    let verb = attr_name(&attr).and_then(|name| verb_method(&name))?;
    Some((verb, attr))
}

/// Quita y devuelve los `#[middleware(…)]` de una lista de atributos (§11.4).
fn take_middlewares(attrs: &mut Vec<Attribute>, errors: &mut Vec<syn::Error>) -> Vec<Expr> {
    let mut found = Vec::new();
    let mut keep = Vec::new();
    for attr in attrs.drain(..) {
        if attr_name(&attr).as_deref() == Some("middleware") {
            match parse_middleware(&attr) {
                Ok(expr) => found.push(expr),
                Err(err) => errors.push(err),
            }
        } else {
            keep.push(attr);
        }
    }
    *attrs = keep;
    found
}

/// `#[middleware(expr)]`: cualquier expresión que produzca `impl Middleware` (RF-086).
fn parse_middleware(attr: &Attribute) -> syn::Result<Expr> {
    let Meta::List(list) = &attr.meta else {
        return Err(syn::Error::new_spanned(
            attr,
            "RDD0401: `#[middleware]` espera una expresión entre paréntesis, p. ej. \
             `#[middleware(Authenticated)]` (§11.4)",
        ));
    };
    list.parse_args::<Expr>().map_err(|err| {
        syn::Error::new(
            err.span(),
            format!(
                "RDD0401: el contenido de `#[middleware(…)]` debe ser una expresión que produzca \
                 un `impl rdd::Middleware` (§11.4)\n Detalle del parser: {err}"
            ),
        )
    })
}

fn parse_verb_args(attr: &Attribute) -> syn::Result<VerbArgs> {
    let Meta::List(list) = &attr.meta else {
        return Err(syn::Error::new_spanned(
            attr,
            "RDD0303: el atributo de verbo necesita la ruta, p. ej. `#[get(\"/tasks\")]` (§8.2)",
        ));
    };
    list.parse_args::<VerbArgs>().map_err(|err| {
        syn::Error::new(
            err.span(),
            format!(
                "RDD0303: la ruta del verbo debe ser una cadena literal que empiece por `/`, con \
                 parámetros `/:nombre` (§8.2, §10.2)\n Detalle del parser: {err}"
            ),
        )
    })
}

/// Construye el `Route` de un handler: valida firma, estado y extractores.
fn route_for(
    sig: &Signature,
    verb: &'static str,
    path: &str,
    args: VerbArgs,
    middlewares: Vec<Expr>,
    self_ty: &Type,
    attr_span: Span,
) -> Result<Route, Vec<syn::Error>> {
    let mut errors = Vec::new();
    let handler = sig.ident.clone();
    let params = route_params(path);
    let mut extracts = Vec::new();
    let mut arguments = Vec::new();

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(receiver) => errors.push(syn::Error::new_spanned(
                receiver,
                "RDD0302: los handlers de controller son funciones asociadas, no métodos con \
                 `self` (RF-050)\n  help: quita `self`; el estado va en los servicios \
                 inyectados como parámetros",
            )),
            FnArg::Typed(typed) => match &*typed.pat {
                Pat::Ident(binding) => {
                    let ident = &binding.ident;
                    let ty = &typed.ty;
                    let bind = bind_for(ty, ident, &params);
                    extracts.push(quote_spanned! { ty.span() =>
                        let #ident = ::rdd::api::extract::<#ty>(&__rdd_ctx, #bind).await?;
                    });
                    arguments.push(ident.clone());
                }
                other => errors.push(syn::Error::new_spanned(
                    other,
                    "RDD0302: cada parámetro de un handler debe ser un identificador \
                     (`svc: TaskService`), no un patrón (RF-050)",
                )),
            },
        }
    }

    let return_type = match &sig.output {
        ReturnType::Default => {
            errors.push(syn::Error::new_spanned(
                &sig.ident,
                "RDD0305: los handlers deben devolver `rdd::Result<T>` para que el núcleo pueda \
                 convertirlo en respuesta (RF-052, §10.3)",
            ));
            None
        }
        ReturnType::Type(_, ty) => match result_inner(ty) {
            Some(inner) => Some(inner),
            None => {
                errors.push(syn::Error::new_spanned(
                    ty,
                    "RDD0305: los handlers deben devolver `rdd::Result<T>` (§8.7)\n  help: \
                     `-> Result<Task>` para un cuerpo JSON, `-> Result<()>` si no hay respuesta",
                ));
                None
            }
        },
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    let is_unit = matches!(&return_type, Some(inner) if is_unit_type(inner));
    let status = status_tokens(args.status, args.status_span, verb, is_unit);
    let method = format_ident!("{}", verb);
    let awaits = if sig.asyncness.is_some() {
        quote!(.await)
    } else {
        quote!()
    };
    let call = quote_spanned! { attr_span =>
        #self_ty::#handler(#(#arguments),*) #awaits
    };

    Ok(Route {
        handler,
        method: quote! { ::rdd::http::Method::#method },
        path: path.to_owned(),
        status,
        middlewares,
        extracts,
        call,
        attr_span,
    })
}

/// Estado base que pasa la macro al handler (RF-053).
fn status_tokens(explicit: Option<u16>, span: Span, verb: &str, is_unit: bool) -> TokenStream {
    if let Some(code) = explicit {
        // El rango ya se validó en `VerbArgs::parse`, así que el `expect` no se dispara.
        return quote_spanned! { span =>
            ::rdd::http::StatusCode::from_u16(#code).expect("código de estado fuera de rango")
        };
    }
    if is_unit {
        return quote! { ::rdd::http::StatusCode::NO_CONTENT };
    }
    if verb == "POST" {
        return quote! { ::rdd::http::StatusCode::CREATED };
    }
    quote! { ::rdd::http::StatusCode::OK }
}

/// Nombres de los parámetros de ruta declarados (`/:id` → `id`), en orden (§10.2).
fn route_params(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix(':'))
        .filter(|name| !name.is_empty())
        .map(|name| name.to_owned())
        .collect()
}

/// Pista de enlace que la macro pasa a cada extractor (RF-054, RF-057).
fn bind_for(ty: &Type, ident: &Ident, params: &[String]) -> TokenStream {
    let name = extractor_name(ty);
    match name.as_deref() {
        Some("Path") => {
            // El nombre del parámetro de ruta se busca en la firma: si el
            // identificador coincide se usa ese; con un solo parámetro de ruta no
            // hay ambigüedad; si no, se propaga el ident y el fallo llega como 400
            // nombrándolo (RF-055).
            let ident_name = ident.to_string();
            let declared = params.iter().find(|param| **param == ident_name);
            let single = params.len() == 1;
            let param = match declared {
                Some(param) => Some(param.clone()),
                None if single => Some(params[0].clone()),
                None => None,
            }
            .unwrap_or(ident_name);
            quote! { ::rdd::http::Bind::param(#param) }
        }
        Some("Header") => {
            let header = ident.to_string().replace('_', "-");
            quote! { ::rdd::http::Bind::header(#header) }
        }
        // `Json`, `Query`, `Ext`, services y `Config<T>` no necesitan enlace.
        _ => quote! { ::rdd::http::Bind::none() },
    }
}

/// Último segmento del tipo del parámetro, para reconocer el extractor.
fn extractor_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    path.path.segments.last().map(|s| s.ident.to_string())
}

/// Tipo interior de `Result<T>`; `None` si la firma no tiene esa forma (RDD0305).
fn result_inner(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    }
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Emite el `impl` original, las funciones de ruta y los registros del módulo.
fn render(imp: ItemImpl, routes: Vec<Route>) -> TokenStream {
    let self_ty = &imp.self_ty;
    let helpers = routes.iter().map(route_helper);
    let push_all = routes.iter().map(|route| {
        let helper = format_ident!("__rdd_route_{}", route.handler);
        quote! { __rdd_router.push(#self_ty::#helper()); }
    });
    let route_list = routes.iter().map(|route| {
        let helper = format_ident!("__rdd_route_{}", route.handler);
        quote! { #self_ty::#helper() }
    });
    let type_name = type_label(self_ty);

    let generated = quote! {
        #imp

        impl #self_ty {
            #(#helpers)*

            /// Las rutas del controller, ya materializadas (RF-056).
            ///
            /// Es el único código público que genera `#[controller]` además de los
            /// propios handlers (§8.5); se puede montar a mano con `.push` si hace
            /// falta inspeccionarlas.
            pub fn routes() -> ::std::vec::Vec<::rdd::http::RouteDef> {
                ::std::vec![ #(#route_list,)* ]
            }
        }

        impl ::rdd::Controller for #self_ty {
            const NAME: &'static str = #type_name;

            fn register(__rdd_router: &mut ::rdd::api::Router) {
                #(#push_all)*
            }
        }

        impl ::rdd::ModuleMember for #self_ty {
            fn register(__rdd_builder: ::rdd::ModuleBuilder) -> ::rdd::ModuleBuilder {
                __rdd_builder.controller::<#self_ty>()
            }
        }
    };
    generated
}

/// `fn __rdd_route_<handler>() -> RouteDef` de un handler (§8.5).
fn route_helper(route: &Route) -> TokenStream {
    let helper = format_ident!("__rdd_route_{}", route.handler);
    let method = &route.method;
    let path = &route.path;
    let status = &route.status;
    let extracts = &route.extracts;
    let call = &route.call;
    let middlewares = route.middlewares.iter().map(|expr| {
        quote_spanned! { expr.span() =>
            ::std::sync::Arc::new(#expr) as ::std::sync::Arc<dyn ::rdd::http::Middleware>
        }
    });
    // Sin parámetros no hay nada que pedirle al contexto, y evitar el binding
    // ahorra el aviso de variable sin usar en el código del usuario.
    let ctx = if route.extracts.is_empty() {
        quote!(_: ::rdd::http::Ctx)
    } else {
        quote!(__rdd_ctx: ::rdd::http::Ctx)
    };

    quote_spanned! { route.attr_span =>
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #helper() -> ::rdd::http::RouteDef {
            ::rdd::api::route_def(
                #method,
                #path,
                #status,
                ::std::vec![ #(#middlewares,)* ],
                |#ctx| async move {
                    #(#extracts)*
                    #call
                },
            )
        }
    }
}

/// Diagnóstico con span propio, sin tocar el AST del usuario.
fn error_at(span: Span, message: &str) -> TokenStream {
    syn::Error::new(span, message).to_compile_error()
}

/// `syn::Error::combine` se aplica sobre el receptor, así que el primer error
/// acumula al resto.
fn combine(mut errors: Vec<syn::Error>) -> syn::Error {
    let mut acc = errors.remove(0);
    for err in errors {
        acc.combine(err);
    }
    acc
}
