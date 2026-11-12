//! # rdd-graphql
//!
//! Motor GraphQL propio de `rdd` (§10.9, ADR-26). El esquema es el tipo Rust:
//! las macros generan los metadatos y este crate aporta documento, validación,
//! ejecución, introspección, SSE y el explorer embebido.
//!
//! La pieza es neutral respecto al servidor: habla
//! [`rdd_core::http::Request`]/[`rdd_core::http::Response`] y una cola de
//! `Bytes`. El montaje `/graphql` vive en `rdd-axum` (RP-030, RP-031).

mod execute;
mod http;
mod introspection;
mod parser;
mod schema;
mod types;
mod validate;

/// Anidamiento máximo por defecto de los conjuntos de selección (`RF-242`).
pub const DEFAULT_MAX_DEPTH: usize = 16;

pub use http::{GraphqlHttp, GraphqlHttpConfig, GraphqlHttpResponse, SseStream};
pub use schema::{GraphqlContext, GraphqlRequest, GraphqlStream, Schema, SchemaBuilder};
pub use types::{
    ArgDef, FieldDef, GraphqlInput, GraphqlObjectType, GraphqlOutput, GraphqlRoot, GraphqlValue,
    InputFieldDef, InputObjectDef, ObjectTypeDef, ResolveFn, RootField, RootKind, SelectedField,
    SelectionSet, SubscribeFn, TypeRef, TypeRegistry, argument, resolved, serialize_typename,
};
