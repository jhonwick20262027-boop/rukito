//! Expansión de `#[derive(GraphqlObject)]`, `#[derive(GraphqlInput)]` y
//! `#[graphql_object(query | mutation | subscription)]` (§10.9, RF-240).
//!
//! El esquema es el tipo Rust (ADR-27): la macro publica metadatos y un
//! serializador sincrónico. Todo lo generado usa rutas absolutas `::rdd::…`
//! (RN-012) y el crate no depende de `rdd-core` (RA-010).

use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, FieldsNamed, FnArg, ImplItem, ItemImpl, Lit,
    LitStr, Meta, Pat, ReturnType, Type,
};

/// Atributos `#[graphql(...)]` ya interpretados.
#[derive(Default)]
struct GraphqlAttrs {
    name: Option<(String, Span)>,
    skip: Option<Span>,
}

/// `#[derive(GraphqlObject)]`.
pub(crate) fn expand_object(input: DeriveInput) -> TokenStream {
    let self_ty = &input.ident;
    let fields = match named_fields(&input, "GraphqlObject") {
        Ok(fields) => fields,
        Err(err) => return err.to_compile_error(),
    };
    let mut errors = Vec::new();
    if !input.generics.params.is_empty() {
        errors.push(syn::Error::new_spanned(
            &input.generics,
            "RDD0501: un tipo con `GraphqlObject` no admite genéricos ni lifetimes (§10.9)\n  \
             help: publica un tipo concreto y deja la genericidad del lado del resolutor",
        ));
    }
    let attrs = parse_attrs(&input.attrs, &mut errors);
    let (name, name_span) = split_name(attrs.name, self_ty.to_string());
    if let Some(span) = name_span
        && let Some(err) = reserved(&name, span, "el tipo")
    {
        errors.push(err);
    }
    let description = documentation(&input.attrs);

    let mut defs = Vec::new();
    let mut arms = Vec::new();
    let mut collects = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for field in &fields.named {
        let field_attrs = parse_attrs(&field.attrs, &mut errors);
        if field_attrs.skip.is_some() {
            continue;
        }
        let ident = field.ident.as_ref().expect("campos con nombre");
        let published = field_attrs
            .name
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| camel_case(&ident.to_string()));
        if let Some(span) = field_attrs.name.as_ref().map(|(_, span)| *span)
            && let Some(err) = reserved(&published, span, "el campo")
        {
            errors.push(err);
        }
        if !seen.insert(published.clone()) {
            errors.push(syn::Error::new_spanned(
                ident,
                format!(
                    "RDD0502: el campo `{published}` está duplicado en `{name}` (§10.9)\n  \
                     help: publica uno de los dos con `#[graphql(name = \"…\")]`"
                ),
            ));
        }
        if let Err(err) = check_type(&field.ty, &name, &published, "salida") {
            errors.push(err);
        }
        let ty = &field.ty;
        let text = description_tokens(documentation(&field.attrs));
        defs.push(quote! {
            ::rdd::graphql::FieldDef {
                name: #published,
                description: #text,
                ty: <#ty as ::rdd::graphql::GraphqlOutput>::type_ref(),
                args: ::std::vec::Vec::new(),
            }
        });
        arms.push(quote! {
            #published => ::rdd::graphql::GraphqlOutput::serialize(
                &self.#ident,
                &__rdd_field.selection,
            ),
        });
        collects.push(quote! {
            <#ty as ::rdd::graphql::GraphqlOutput>::collect_types(types);
        });
    }

    if !errors.is_empty() {
        return combine(errors).to_compile_error();
    }

    let text = description_tokens(description);
    quote! {
        impl ::rdd::graphql::GraphqlOutput for #self_ty {
            fn type_ref() -> ::rdd::graphql::TypeRef {
                ::rdd::graphql::TypeRef::required(#name)
            }

            fn collect_types(types: &mut ::rdd::graphql::TypeRegistry) {
                types.object(
                    #name,
                    #text,
                    || ::std::vec![#(#defs),*],
                    |types| {
                        #(#collects)*
                    },
                );
            }

            fn serialize(
                &self,
                __rdd_selection: &::rdd::graphql::SelectionSet,
            ) -> ::rdd::serde_json::Value {
                let mut __rdd_out = ::rdd::serde_json::Map::new();
                for __rdd_field in &__rdd_selection.fields {
                    let __rdd_value = match __rdd_field.name.as_str() {
                        #(#arms)*
                        _ => continue,
                    };
                    __rdd_out.insert(__rdd_field.key.clone(), __rdd_value);
                }
                ::rdd::graphql::serialize_typename(&mut __rdd_out, __rdd_selection, #name);
                ::rdd::serde_json::Value::Object(__rdd_out)
            }
        }

        impl ::rdd::graphql::GraphqlObjectType for #self_ty {
            fn object_name() -> &'static str {
                #name
            }

            fn object_description() -> ::std::option::Option<&'static str> {
                #text
            }

            fn object_fields() -> ::std::vec::Vec<::rdd::graphql::FieldDef> {
                ::std::vec![#(#defs),*]
            }
        }
    }
}

/// `#[derive(GraphqlInput)]`.
pub(crate) fn expand_input(input: DeriveInput) -> TokenStream {
    let self_ty = &input.ident;
    let fields = match named_fields(&input, "GraphqlInput") {
        Ok(fields) => fields,
        Err(err) => return err.to_compile_error(),
    };
    let mut errors = Vec::new();
    if !input.generics.params.is_empty() {
        errors.push(syn::Error::new_spanned(
            &input.generics,
            "RDD0501: un tipo con `GraphqlInput` no admite genéricos ni lifetimes (§10.9)\n  \
             help: publica un tipo concreto y deja la genericidad del lado del resolutor",
        ));
    }
    let attrs = parse_attrs(&input.attrs, &mut errors);
    let (name, name_span) = split_name(attrs.name, self_ty.to_string());
    if let Some(span) = name_span
        && let Some(err) = reserved(&name, span, "el tipo")
    {
        errors.push(err);
    }
    let description = documentation(&input.attrs);

    let mut defs = Vec::new();
    let mut inits = Vec::new();
    let mut collects = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for field in &fields.named {
        let field_attrs = parse_attrs(&field.attrs, &mut errors);
        if let Some(span) = field_attrs.skip {
            errors.push(syn::Error::new(
                span,
                "RDD0501: `#[graphql(skip)]` no se admite en `GraphqlInput` (§10.9)\n  \
                 help: un campo de entrada siempre debe poder leerse del documento",
            ));
            continue;
        }
        let ident = field.ident.as_ref().expect("campos con nombre");
        let published = field_attrs
            .name
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| camel_case(&ident.to_string()));
        if let Some(span) = field_attrs.name.as_ref().map(|(_, span)| *span)
            && let Some(err) = reserved(&published, span, "el campo")
        {
            errors.push(err);
        }
        if !seen.insert(published.clone()) {
            errors.push(syn::Error::new_spanned(
                ident,
                format!(
                    "RDD0502: el campo `{published}` está duplicado en `{name}` (§10.9)\n  \
                     help: publica uno de los dos con `#[graphql(name = \"…\")]`"
                ),
            ));
        }
        if let Err(err) = check_type(&field.ty, &name, &published, "entrada") {
            errors.push(err);
        }
        let ty = &field.ty;
        let text = description_tokens(documentation(&field.attrs));
        defs.push(quote! {
            ::rdd::graphql::InputFieldDef {
                name: #published,
                description: #text,
                ty: <#ty as ::rdd::graphql::GraphqlInput>::type_ref(),
                default: ::std::option::Option::None,
            }
        });
        inits.push(quote! {
            #ident: <#ty as ::rdd::graphql::GraphqlInput>::from_json(
                __rdd_map
                    .get(#published)
                    .unwrap_or(&::rdd::serde_json::Value::Null),
            )
            .map_err(|__rdd_err| ::std::format!("campo `{}`: {}", #published, __rdd_err))?,
        });
        collects.push(quote! {
            <#ty as ::rdd::graphql::GraphqlInput>::collect_types(types);
        });
    }

    if !errors.is_empty() {
        return combine(errors).to_compile_error();
    }

    let text = description_tokens(description);
    quote! {
        impl ::rdd::graphql::GraphqlInput for #self_ty {
            fn type_ref() -> ::rdd::graphql::TypeRef {
                ::rdd::graphql::TypeRef::required(#name)
            }

            fn collect_types(types: &mut ::rdd::graphql::TypeRegistry) {
                types.input(
                    #name,
                    #text,
                    || ::std::vec![#(#defs),*],
                    |types| {
                        #(#collects)*
                    },
                );
            }

            fn from_json(
                __rdd_value: &::rdd::serde_json::Value,
            ) -> ::std::result::Result<Self, ::std::string::String> {
                let ::rdd::serde_json::Value::Object(__rdd_map) = __rdd_value else {
                    return ::std::result::Result::Err(::std::format!(
                        "se esperaba un objeto `{}`",
                        #name,
                    ));
                };
                ::std::result::Result::Ok(Self {
                    #(#inits)*
                })
            }
        }
    }
}

/// `#[graphql_object(query | mutation | subscription)]`.
pub(crate) fn expand_root(args: TokenStream, item: TokenStream) -> TokenStream {
    let imp: ItemImpl = match syn::parse2(item) {
        Ok(imp) => imp,
        Err(err) => return err.to_compile_error(),
    };
    let mut errors = Vec::new();
    let kind = match parse_kind(args) {
        Ok(kind) => kind,
        Err(err) => return err.to_compile_error(),
    };
    if imp.trait_.is_some() {
        errors.push(syn::Error::new_spanned(
            &imp.self_ty,
            "RDD0501: `#[graphql_object(...)]` se aplica a un `impl` inherente (§10.9)\n  \
             help: publica los campos como métodos de la raíz, no como una implementación de trait",
        ));
    }
    let self_ty = imp.self_ty.clone();
    let self_ty = &self_ty;
    let subscription = kind == "subscription";
    let kind_tokens = match kind {
        "query" => quote!(::rdd::graphql::RootKind::Query),
        "mutation" => quote!(::rdd::graphql::RootKind::Mutation),
        _ => quote!(::rdd::graphql::RootKind::Subscription),
    };

    let mut fields = Vec::new();
    let mut collects = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for item in &imp.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        let attrs = parse_attrs(&method.attrs, &mut errors);
        if attrs.skip.is_some() {
            continue;
        }
        let ident = &method.sig.ident;
        let published = attrs
            .name
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| camel_case(&ident.to_string()));
        if let Some(span) = attrs.name.as_ref().map(|(_, span)| *span)
            && let Some(err) = reserved(&published, span, "el campo")
        {
            errors.push(err);
        }
        if !seen.insert(published.clone()) {
            errors.push(syn::Error::new_spanned(
                ident,
                format!(
                    "RDD0502: el campo `{published}` está duplicado en la raíz `{kind}` (§10.9)\n  \
                     help: publica uno de los dos con `#[graphql(name = \"…\")]`"
                ),
            ));
        }
        if method.sig.asyncness.is_none() {
            errors.push(syn::Error::new_spanned(
                &method.sig,
                format!(
                    "RDD0501: el campo `{published}` debe ser `async fn` (§10.9)\n  \
                     help: los resolutores raíz se ejecutan sin bloquear el runtime"
                ),
            ));
            continue;
        }
        if !method.sig.generics.params.is_empty() {
            errors.push(syn::Error::new_spanned(
                &method.sig.generics,
                format!("RDD0501: el campo `{published}` no admite genéricos ni lifetimes (§10.9)"),
            ));
            continue;
        }

        let mut inputs = method.sig.inputs.iter();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            errors.push(syn::Error::new_spanned(
                &method.sig,
                format!(
                    "RDD0501: el campo `{published}` debe recibir `&self` como primer parámetro (§10.9)"
                ),
            ));
            continue;
        };
        if receiver.reference.is_none() || receiver.mutability.is_some() {
            errors.push(syn::Error::new_spanned(
                receiver,
                format!(
                    "RDD0501: el campo `{published}` debe recibir `&self`, no `self` ni `&mut self` (§10.9)"
                ),
            ));
            continue;
        }

        let Some(FnArg::Typed(context)) = inputs.next() else {
            errors.push(syn::Error::new_spanned(
                &method.sig,
                format!(
                    "RDD0501: el campo `{published}` debe recibir `&GraphqlContext` después de `&self` (§10.9)"
                ),
            ));
            continue;
        };
        if !is_context(&context.ty) {
            errors.push(syn::Error::new_spanned(
                &context.ty,
                format!(
                    "RDD0501: el segundo parámetro del campo `{published}` debe ser `&GraphqlContext` (§10.9)"
                ),
            ));
            continue;
        }

        let Some(inner) = result_inner(&method.sig.output) else {
            errors.push(syn::Error::new_spanned(
                &method.sig.output,
                format!("RDD0501: el campo `{published}` debe devolver `rdd::Result<T>` (§10.9)"),
            ));
            continue;
        };
        let output_ty = if subscription {
            let Some(stream) = generic_inner(inner, "GraphqlStream") else {
                errors.push(syn::Error::new_spanned(
                    inner,
                    "RDD0501: un campo de suscripción debe devolver \
                     `rdd::Result<GraphqlStream<T>>` (§10.9)",
                ));
                continue;
            };
            stream.clone()
        } else {
            inner.clone()
        };

        let mut arg_defs = Vec::new();
        let mut extracts = Vec::new();
        let mut arg_idents = Vec::new();
        let mut valid = true;
        for (index, input) in inputs.enumerate() {
            let FnArg::Typed(input) = input else {
                errors.push(syn::Error::new_spanned(
                    input,
                    format!("RDD0501: el campo `{published}` no admite más receptores (§10.9)"),
                ));
                valid = false;
                break;
            };
            let Pat::Ident(pattern) = &*input.pat else {
                errors.push(syn::Error::new_spanned(
                    &input.pat,
                    format!(
                        "RDD0501: el argumento {} del campo `{published}` debe ser un nombre (§10.9)",
                        index + 1
                    ),
                ));
                valid = false;
                break;
            };
            let arg_ident = &pattern.ident;
            let arg_name = camel_case(&arg_ident.to_string());
            let arg_ty = &input.ty;
            if let Err(err) = check_type(arg_ty, "el campo", &published, "entrada") {
                errors.push(err);
            }
            let text = description_tokens(documentation(&input.attrs));
            arg_defs.push(quote! {
                ::rdd::graphql::ArgDef {
                    name: #arg_name,
                    description: #text,
                    ty: <#arg_ty as ::rdd::graphql::GraphqlInput>::type_ref(),
                }
            });
            extracts.push(quote! {
                let #arg_ident =
                    ::rdd::graphql::argument::<#arg_ty>(&__rdd_args, #arg_name)?;
            });
            collects.push(quote! {
                <#arg_ty as ::rdd::graphql::GraphqlInput>::collect_types(types);
            });
            arg_idents.push(arg_ident.clone());
        }
        if !valid {
            continue;
        }

        collects.push(quote! {
            <#output_ty as ::rdd::graphql::GraphqlOutput>::collect_types(types);
        });

        let text = description_tokens(documentation(&method.attrs));
        let resolver = if subscription {
            quote! {
                resolve: ::std::option::Option::None,
                subscribe: ::std::option::Option::Some(::std::sync::Arc::new(
                    |__rdd_ctx: &::rdd::graphql::GraphqlContext,
                     __rdd_args: ::rdd::serde_json::Value| {
                        let __rdd_ctx = ::std::clone::Clone::clone(__rdd_ctx);
                        ::std::boxed::Box::pin(async move {
                            let __rdd_self =
                                <#self_ty as ::std::default::Default>::default();
                            #(#extracts)*
                            let __rdd_stream =
                                __rdd_self.#ident(&__rdd_ctx, #(#arg_idents),*).await?;
                            ::std::result::Result::Ok(
                                ::rdd::graphql::GraphqlStream::into_mapped(__rdd_stream),
                            )
                        })
                    },
                )),
            }
        } else {
            quote! {
                resolve: ::std::option::Option::Some(::std::sync::Arc::new(
                    |__rdd_ctx: &::rdd::graphql::GraphqlContext,
                     __rdd_args: ::rdd::serde_json::Value| {
                        let __rdd_ctx = ::std::clone::Clone::clone(__rdd_ctx);
                        ::std::boxed::Box::pin(async move {
                            let __rdd_self =
                                <#self_ty as ::std::default::Default>::default();
                            #(#extracts)*
                            let __rdd_value =
                                __rdd_self.#ident(&__rdd_ctx, #(#arg_idents),*).await?;
                            ::std::result::Result::Ok(
                                ::rdd::graphql::resolved(__rdd_value),
                            )
                        })
                    },
                )),
                subscribe: ::std::option::Option::None,
            }
        };

        fields.push(quote! {
            ::rdd::graphql::RootField {
                name: #published,
                description: #text,
                args: ::std::vec![#(#arg_defs),*],
                ty: <#output_ty as ::rdd::graphql::GraphqlOutput>::type_ref(),
                kind: #kind_tokens,
                #resolver
            }
        });
    }

    if !errors.is_empty() {
        return combine(errors).to_compile_error();
    }

    let mut imp = imp;
    for item in &mut imp.items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| !attr.path().is_ident("graphql"));
        }
    }

    quote! {
        #imp

        impl ::rdd::graphql::GraphqlRoot for #self_ty {
            fn root_fields() -> ::std::vec::Vec<::rdd::graphql::RootField> {
                ::std::vec![#(#fields),*]
            }

            fn collect_root_types(types: &mut ::rdd::graphql::TypeRegistry) {
                #(#collects)*
            }
        }
    }
}

fn named_fields<'a>(
    input: &'a DeriveInput,
    macro_name: &str,
) -> Result<&'a FieldsNamed, syn::Error> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            format!("RDD0501: `{macro_name}` solo se aplica a structs (§10.9)"),
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            format!(
                "RDD0501: `{macro_name}` exige campos con nombre (§10.9)\n  \
                 help: los campos publicados son las claves del objeto GraphQL"
            ),
        ));
    };
    Ok(fields)
}

fn parse_kind(args: TokenStream) -> Result<&'static str, syn::Error> {
    let ident: syn::Ident = syn::parse2(args).map_err(|_| {
        syn::Error::new(
            Span::call_site(),
            "RDD0501: `#[graphql_object(...)]` espera `query`, `mutation` o `subscription` \
             (§10.9)",
        )
    })?;
    match ident.to_string().as_str() {
        "query" => Ok("query"),
        "mutation" => Ok("mutation"),
        "subscription" => Ok("subscription"),
        other => Err(syn::Error::new(
            ident.span(),
            format!(
                "RDD0501: `{other}` no es una raíz GraphQL; se admite `query`, `mutation` \
                 o `subscription` (§10.9)"
            ),
        )),
    }
}

fn split_name(name: Option<(String, Span)>, fallback: String) -> (String, Option<Span>) {
    match name {
        Some((name, span)) => (name, Some(span)),
        None => (fallback, None),
    }
}

fn parse_attrs(attrs: &[Attribute], errors: &mut Vec<syn::Error>) -> GraphqlAttrs {
    let mut parsed = GraphqlAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("graphql") {
            continue;
        }
        let result = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let lit: LitStr = meta.value()?.parse()?;
                parsed.name = Some((lit.value(), lit.span()));
                Ok(())
            } else if meta.path.is_ident("skip") {
                parsed.skip = Some(meta.path.span());
                Ok(())
            } else {
                Err(meta.error("atributo `graphql` desconocido: se admite `name = \"…\"` y `skip`"))
            }
        });
        if let Err(err) = result {
            errors.push(err);
        }
    }
    parsed
}

fn documentation(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(value) = &attr.meta else {
            continue;
        };
        let Expr::Lit(expr) = &value.value else {
            continue;
        };
        let Lit::Str(text) = &expr.lit else {
            continue;
        };
        lines.push(text.value().trim().to_owned());
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn description_tokens(text: Option<String>) -> TokenStream {
    match text {
        Some(text) => quote!(::std::option::Option::Some(#text)),
        None => quote!(::std::option::Option::None),
    }
}

fn reserved(name: &str, span: Span, owner: &str) -> Option<syn::Error> {
    name.starts_with("__").then(|| {
        syn::Error::new(
            span,
            format!(
                "RDD0502: {owner} `{name}` usa el prefijo reservado `__` (§10.9)\n  \
                 help: reserva `__` para la introspección y publica otro nombre"
            ),
        )
    })
}

/// Rechaza los tipos que el motor no publica (`i64`, `u64`, referencias…).
fn check_type(ty: &Type, owner: &str, field: &str, direction: &str) -> Result<(), syn::Error> {
    if contains_reference(ty) {
        return Err(syn::Error::new_spanned(
            ty,
            format!(
                "RDD0501: el campo `{field}` de `{owner}` usa un tipo prestado (§10.9)\n  \
                 help: los campos GraphQL son datos con vida propia; usa un tipo sin lifetimes"
            ),
        ));
    }
    let Some(named) = named_type(ty) else {
        return Err(syn::Error::new_spanned(
            ty,
            format!(
                "RDD0501: el tipo del campo `{field}` de `{owner}` no está soportado como \
                 {direction} (§10.9)"
            ),
        ));
    };
    match named.as_str() {
        "i64" | "u64" | "i128" | "u128" | "isize" | "usize" | "f32" => {
            Err(syn::Error::new_spanned(
                ty,
                format!(
                    "RDD0501: el campo `{field}` de `{owner}` usa `{named}`, que no tiene escalar \
                 GraphQL (§10.9)\n  \
                 help: publica `i32`, `f64` o `String`; los enteros de 64 bits no viajan \
                 en la introspección"
                ),
            ))
        }
        _ => Ok(()),
    }
}

fn contains_reference(ty: &Type) -> bool {
    match ty {
        Type::Reference(_) => true,
        Type::Path(path) => path.path.segments.iter().any(|segment| {
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return false;
            };
            args.args.iter().any(|arg| match arg {
                syn::GenericArgument::Type(inner) => contains_reference(inner),
                syn::GenericArgument::Lifetime(_) => true,
                _ => false,
            })
        }),
        Type::Tuple(tuple) => tuple.elems.iter().any(contains_reference),
        Type::Group(group) => contains_reference(&group.elem),
        Type::Paren(paren) => contains_reference(&paren.elem),
        _ => false,
    }
}

/// Nombre del tipo más interno, despojando `Option<T>` y `Vec<T>`.
fn named_type(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    let name = last.ident.to_string();
    if name == "Option" || name == "Vec" {
        return generic_inner(ty, &name).and_then(named_type);
    }
    Some(name)
}

fn generic_inner<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != name {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

fn result_inner(output: &ReturnType) -> Option<&Type> {
    let ReturnType::Type(_, ty) = output else {
        return None;
    };
    let Type::Path(path) = &**ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

fn is_context(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Path(path) = &*reference.elem else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "GraphqlContext")
}

fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for character in name.chars() {
        if character == '_' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(character.to_uppercase());
            upper = false;
        } else {
            out.push(character);
        }
    }
    out
}

fn combine(mut errors: Vec<syn::Error>) -> syn::Error {
    let mut accumulated = errors.remove(0);
    for error in errors {
        accumulated.combine(error);
    }
    accumulated
}
