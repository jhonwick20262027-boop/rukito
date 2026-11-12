//! `Schema`, `SchemaBuilder` y el contexto de ejecución (§10.9).
//!
//! El esquema se valida **al construir** (`RF-244`): un choque de nombres, un
//! tipo sin definir o una raíz vacía impiden arrancar y el mensaje nombra el
//! tipo y el campo.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::parser::{self, Document, Operation, OperationKind};
use crate::types::{
    ArgDef, FieldDef, GraphqlOutput, GraphqlRoot, InputObjectDef, ObjectTypeDef, RootField,
    RootKind, TypeRef, TypeRegistry,
};

/// Escalares publicados en la introspección (`RF-237`).
pub const SCALARS: [&str; 5] = ["Int", "Float", "String", "Boolean", "ID"];

/// Tipos del esquema ya validados e inmutables.
#[derive(Default)]
pub struct Schema {
    query: Option<Root>,
    mutation: Option<Root>,
    subscription: Option<Root>,
    objects: BTreeMap<String, ObjectTypeDef>,
    inputs: BTreeMap<String, InputObjectDef>,
}

#[derive(Clone, Debug)]
pub(crate) struct Root {
    pub fields: Vec<RootField>,
}

/// Constructor del esquema (`RF-240`, `RF-244`).
#[derive(Default)]
pub struct SchemaBuilder {
    query: Option<RootEntry>,
    mutation: Option<RootEntry>,
    subscription: Option<RootEntry>,
}

struct RootEntry {
    fields: Vec<RootField>,
    collect: fn(&mut TypeRegistry),
}

fn collect_root<R: GraphqlRoot>(types: &mut TypeRegistry) {
    R::collect_root_types(types);
}

impl SchemaBuilder {
    /// Raíz de consulta.
    pub fn query<R: GraphqlRoot>(mut self, _root: R) -> Self {
        self.query = Some(RootEntry {
            fields: R::root_fields(),
            collect: collect_root::<R>,
        });
        self
    }

    /// Raíz de mutación.
    pub fn mutation<R: GraphqlRoot>(mut self, _root: R) -> Self {
        self.mutation = Some(RootEntry {
            fields: R::root_fields(),
            collect: collect_root::<R>,
        });
        self
    }

    /// Raíz de suscripción.
    pub fn subscription<R: GraphqlRoot>(mut self, _root: R) -> Self {
        self.subscription = Some(RootEntry {
            fields: R::root_fields(),
            collect: collect_root::<R>,
        });
        self
    }

    /// Valida nombres, tipos y raíces (`RF-244`).
    pub fn build(self) -> rdd_core::Result<Schema> {
        let mut types = TypeRegistry::default();
        let mut roots: [Option<Root>; 3] = [None, None, None];
        for (index, entry) in [(0, self.query), (1, self.mutation), (2, self.subscription)] {
            let Some(entry) = entry else { continue };
            if entry.fields.is_empty() {
                return Err(startup_error(format!(
                    "la raíz `{}` no expone ningún campo",
                    match index {
                        0 => "Query",
                        1 => "Mutation",
                        _ => "Subscription",
                    }
                )));
            }
            (entry.collect)(&mut types);
            roots[index] = Some(Root {
                fields: entry.fields,
            });
        }

        if let Some(conflict) = types.conflicts.first() {
            return Err(startup_error(conflict.clone()));
        }

        let [query, mutation, subscription] = roots;
        let mut objects: BTreeMap<String, ObjectTypeDef> = BTreeMap::new();
        for object in types.objects {
            check_type_name(object.name)?;
            if let Some(existing) = objects.get(object.name)
                && existing.fields != object.fields
            {
                return Err(startup_error(format!(
                    "el tipo `{}` se define dos veces con campos distintos",
                    object.name
                )));
            }
            objects.insert(object.name.to_owned(), object);
        }
        let mut inputs: BTreeMap<String, InputObjectDef> = BTreeMap::new();
        for input in types.inputs {
            check_type_name(input.name)?;
            if let Some(existing) = inputs.get(input.name)
                && existing.fields != input.fields
            {
                return Err(startup_error(format!(
                    "el tipo de entrada `{}` se define dos veces con campos distintos",
                    input.name
                )));
            }
            if objects.contains_key(input.name) {
                return Err(startup_error(format!(
                    "`{}` se publica como objeto y como entrada a la vez",
                    input.name
                )));
            }
            inputs.insert(input.name.to_owned(), input);
        }

        for (kind, root) in [
            (RootKind::Query, query.as_ref()),
            (RootKind::Mutation, mutation.as_ref()),
            (RootKind::Subscription, subscription.as_ref()),
        ] {
            let Some(root) = root else { continue };
            let type_name = kind.type_name();
            let mut seen = BTreeMap::new();
            let mut fields = Vec::new();
            for field in &root.fields {
                check_name(field.name, type_name)?;
                if seen.insert(field.name, ()).is_some() {
                    return Err(startup_error(format!(
                        "el campo `{}` está duplicado en `{type_name}`",
                        field.name
                    )));
                }
                let mut args = Vec::new();
                for arg in &field.args {
                    check_name(arg.name, &format!("{type_name}.{}", field.name))?;
                    if args.iter().any(|item: &ArgDef| item.name == arg.name) {
                        return Err(startup_error(format!(
                            "el argumento `{}` está duplicado en `{type_name}.{}`",
                            arg.name, field.name
                        )));
                    }
                    check_input_type(&arg.ty, &inputs)?;
                    args.push(arg.clone());
                }
                check_output_type(&field.ty, &objects)?;
                fields.push(FieldDef {
                    name: field.name,
                    description: field.description,
                    ty: field.ty.clone(),
                    args,
                });
            }
            if objects.contains_key(type_name) {
                return Err(startup_error(format!(
                    "el tipo `{type_name}` choca con una raíz del esquema"
                )));
            }
            objects.insert(
                type_name.to_owned(),
                ObjectTypeDef {
                    name: type_name,
                    description: None,
                    fields,
                },
            );
        }

        check_cross_root_duplicates(&[&query, &mutation, &subscription])?;
        check_object_fields(&objects, &inputs)?;
        check_input_fields(&inputs, &objects)?;

        Ok(Schema {
            query,
            mutation,
            subscription,
            objects,
            inputs,
        })
    }
}

fn startup_error(message: impl Into<String>) -> rdd_core::Error {
    rdd_core::Error::internal(anyhow::anyhow!(message.into()))
}

fn check_type_name(name: &str) -> rdd_core::Result<()> {
    if name.starts_with("__") {
        return Err(startup_error(format!(
            "el tipo `{name}` usa el prefijo reservado `__`"
        )));
    }
    Ok(())
}

fn check_name(name: &str, owner: &str) -> rdd_core::Result<()> {
    if name.starts_with("__") {
        return Err(startup_error(format!(
            "el campo `{name}` de `{owner}` usa el prefijo reservado `__`"
        )));
    }
    Ok(())
}

fn check_output_type(
    ty: &TypeRef,
    objects: &BTreeMap<String, ObjectTypeDef>,
) -> rdd_core::Result<()> {
    let name = ty.named_type();
    if SCALARS.contains(&name) || objects.contains_key(name) {
        Ok(())
    } else {
        Err(startup_error(format!(
            "el tipo de salida `{name}` no está definido"
        )))
    }
}

fn check_input_type(
    ty: &TypeRef,
    inputs: &BTreeMap<String, InputObjectDef>,
) -> rdd_core::Result<()> {
    let name = ty.named_type();
    if SCALARS.contains(&name) || inputs.contains_key(name) {
        Ok(())
    } else {
        Err(startup_error(format!(
            "el tipo de entrada `{name}` no está definido"
        )))
    }
}

fn check_object_fields(
    objects: &BTreeMap<String, ObjectTypeDef>,
    inputs: &BTreeMap<String, InputObjectDef>,
) -> rdd_core::Result<()> {
    for object in objects.values() {
        let mut names = BTreeMap::new();
        for field in &object.fields {
            check_name(field.name, object.name)?;
            if names.insert(field.name, ()).is_some() {
                return Err(startup_error(format!(
                    "el campo `{}` está duplicado en `{}`",
                    field.name, object.name
                )));
            }
            check_output_type(&field.ty, objects)?;
            for arg in &field.args {
                check_input_type(&arg.ty, inputs)?;
            }
        }
    }
    Ok(())
}

fn check_input_fields(
    inputs: &BTreeMap<String, InputObjectDef>,
    objects: &BTreeMap<String, ObjectTypeDef>,
) -> rdd_core::Result<()> {
    for input in inputs.values() {
        let mut names = BTreeMap::new();
        for field in &input.fields {
            check_name(field.name, input.name)?;
            if names.insert(field.name, ()).is_some() {
                return Err(startup_error(format!(
                    "el campo `{}` está duplicado en `{}`",
                    field.name, input.name
                )));
            }
            if objects.contains_key(field.ty.named_type()) {
                return Err(startup_error(format!(
                    "el campo `{}` de `{}` usa un tipo objeto como entrada",
                    field.name, input.name
                )));
            }
            check_input_type(&field.ty, inputs)?;
        }
    }
    Ok(())
}

fn check_cross_root_duplicates(roots: &[&Option<Root>]) -> rdd_core::Result<()> {
    let mut seen: BTreeMap<&str, &'static str> = BTreeMap::new();
    for root in roots {
        let Some(root) = root else { continue };
        for field in &root.fields {
            if let Some(previous) = seen.insert(field.name, field.kind.type_name()) {
                return Err(startup_error(format!(
                    "el campo `{}` se publica en `{}` y en `{}`",
                    field.name,
                    previous,
                    field.kind.type_name()
                )));
            }
        }
    }
    Ok(())
}

impl Schema {
    /// Constructor vacío.
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    /// Ejecuta una consulta o mutación y devuelve `{data, errors}` (`RF-236`).
    pub async fn execute(&self, ctx: &GraphqlContext, req: GraphqlRequest) -> Value {
        match crate::validate::analyze(self, &req, crate::DEFAULT_MAX_DEPTH) {
            Ok(prepared) if prepared.kind != OperationKind::Subscription => {
                crate::execute::execute(self, ctx, prepared).await
            }
            Ok(_) => error_value(vec![crate::validate::GraphqlError::message(
                "una suscripción se atiende con `subscribe`, no con `execute`",
            )]),
            Err(errors) => error_value(errors),
        }
    }

    /// Cierto si la operación seleccionada es una suscripción (`RF-234`).
    pub fn is_subscription(&self, req: &GraphqlRequest) -> bool {
        let Ok(document) = parser::parse(&req.query) else {
            return false;
        };
        matches!(
            select_operation(&document, req.operation_name.as_deref()),
            Ok(operation) if operation.kind == OperationKind::Subscription
        )
    }

    /// Arranca una suscripción (`RF-238`).
    pub async fn subscribe(
        &self,
        ctx: &GraphqlContext,
        req: GraphqlRequest,
    ) -> rdd_core::Result<GraphqlStream<Value>> {
        let prepared =
            crate::validate::analyze(self, &req, crate::DEFAULT_MAX_DEPTH).map_err(|errors| {
                rdd_core::Error::bad_request(
                    errors
                        .first()
                        .map(|error| error.message.clone())
                        .unwrap_or_else(|| "documento inválido".to_owned()),
                )
            })?;
        if prepared.kind != OperationKind::Subscription {
            return Err(rdd_core::Error::bad_request(
                "la operación seleccionada no es una suscripción",
            ));
        }
        crate::execute::subscribe(self, ctx, prepared).await
    }

    pub(crate) fn root(&self, kind: RootKind) -> Option<&Root> {
        match kind {
            RootKind::Query => self.query.as_ref(),
            RootKind::Mutation => self.mutation.as_ref(),
            RootKind::Subscription => self.subscription.as_ref(),
        }
    }

    pub(crate) fn object(&self, name: &str) -> Option<&ObjectTypeDef> {
        self.objects.get(name)
    }

    pub(crate) fn input(&self, name: &str) -> Option<&InputObjectDef> {
        self.inputs.get(name)
    }

    pub(crate) fn type_names(&self) -> Vec<String> {
        let mut names: Vec<String> = SCALARS.iter().map(|name| (*name).to_owned()).collect();
        names.extend(self.objects.keys().cloned());
        names.extend(self.inputs.keys().cloned());
        names.sort();
        names.dedup();
        names
    }
}

/// Petición GraphQL ya normalizada.
pub struct GraphqlRequest {
    /// Documento.
    pub query: String,
    /// Operación elegida cuando el documento trae varias.
    pub operation_name: Option<String>,
    /// Variables.
    pub variables: Option<Value>,
}

impl GraphqlRequest {
    /// Petición sin variables ni nombre de operación.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            operation_name: None,
            variables: None,
        }
    }
}

/// Contexto de una operación (`RF-239`).
///
/// Lleva las extensiones que dejó la cadena global —identidad, contenedor— y
/// permite resolver un provider como en un handler.
#[derive(Clone)]
pub struct GraphqlContext {
    extensions: rdd_core::http::Extensions,
    container: Arc<rdd_core::di::Container>,
}

impl GraphqlContext {
    pub(crate) fn from_request(req: &rdd_core::http::Request) -> Self {
        let container = req
            .extensions()
            .get::<Arc<rdd_core::di::Container>>()
            .cloned()
            .unwrap_or_else(|| Arc::clone(rdd_core::di::default_container()));
        Self {
            extensions: req.extensions().clone(),
            container,
        }
    }

    /// Contexto vacío, para pruebas y ejecuciones sin servidor.
    pub fn empty() -> Self {
        Self {
            extensions: rdd_core::http::Extensions::new(),
            container: Arc::clone(rdd_core::di::default_container()),
        }
    }

    /// Identidad que puso un middleware, p. ej. `ctx.ext::<Session>()`.
    pub fn ext<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.extensions.get::<T>().cloned()
    }

    /// Recurso registrado en el contenedor, sin `FromContainer`.
    pub fn get<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.container.get::<T>()
    }

    /// Provider construido por el contenedor (`#[service]`).
    pub fn resolve<T: rdd_core::http::FromContainer>(&self) -> rdd_core::Result<T> {
        T::from_container(&self.container)
    }
}

/// Flujo de valores de una suscripción (`RF-240`).
pub struct GraphqlStream<T> {
    receiver: mpsc::Receiver<T>,
}

impl<T> GraphqlStream<T> {
    /// Envuelve el receptor que produce una suscripción.
    pub fn new(receiver: mpsc::Receiver<T>) -> Self {
        Self { receiver }
    }

    /// Siguiente elemento, o `None` cuando el producto cerró el flujo.
    #[doc(hidden)]
    pub async fn recv(&mut self) -> Option<T> {
        self.receiver.recv().await
    }
}

impl<T: GraphqlOutput + Send + 'static> GraphqlStream<T> {
    /// Convierte el flujo en uno de valores ya serializables por el motor.
    ///
    /// Lo generan las macros; el usuario no lo llama.
    #[doc(hidden)]
    pub fn into_mapped(self) -> GraphqlStream<crate::GraphqlValue> {
        let (tx, rx) = mpsc::channel(64);
        let mut inner = self;
        tokio::spawn(async move {
            while let Some(item) = inner.recv().await {
                if tx.send(crate::GraphqlValue::new(item)).await.is_err() {
                    break;
                }
            }
        });
        GraphqlStream::new(rx)
    }
}

/// Error de documento, validación o ejecución (`RF-236`).
#[derive(Clone, Debug)]
pub(crate) struct GraphqlError {
    pub message: String,
    pub locations: Vec<parser::Position>,
    pub path: Vec<String>,
}

impl GraphqlError {
    pub(crate) fn at(message: impl Into<String>, position: parser::Position) -> Self {
        Self {
            message: message.into(),
            locations: vec![position],
            path: Vec::new(),
        }
    }

    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            locations: Vec::new(),
            path: Vec::new(),
        }
    }

    pub(crate) fn with_path(mut self, path: Vec<String>) -> Self {
        self.path = path;
        self
    }

    pub(crate) fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("message".to_owned(), Value::String(self.message.clone()));
        if !self.locations.is_empty() {
            let locations = self
                .locations
                .iter()
                .map(|position| {
                    serde_json::json!({
                        "line": position.line,
                        "column": position.column,
                    })
                })
                .collect();
            map.insert("locations".to_owned(), Value::Array(locations));
        }
        if !self.path.is_empty() {
            map.insert(
                "path".to_owned(),
                Value::Array(
                    self.path
                        .iter()
                        .map(|step| Value::String(step.clone()))
                        .collect(),
                ),
            );
        }
        Value::Object(map)
    }
}

/// `{"data": null, "errors": [...]}` para un documento que no se puede ejecutar.
pub(crate) fn error_value(errors: Vec<GraphqlError>) -> Value {
    serde_json::json!({
        "data": Value::Null,
        "errors": errors.iter().map(GraphqlError::to_json).collect::<Vec<_>>(),
    })
}

/// Selecciona la operación que pide el cliente (`RF-234`).
pub(crate) fn select_operation<'a>(
    document: &'a Document,
    operation_name: Option<&str>,
) -> Result<&'a Operation, GraphqlError> {
    if let Some(name) = operation_name {
        return document
            .operations
            .iter()
            .find(|operation| operation.name.as_deref() == Some(name))
            .ok_or_else(|| {
                GraphqlError::message(format!(
                    "no hay ninguna operación llamada `{name}` en el documento"
                ))
            });
    }
    match document.operations.as_slice() {
        [operation] => Ok(operation),
        [] => Err(GraphqlError::message(
            "el documento no contiene operaciones",
        )),
        _ => Err(GraphqlError::message(
            "el documento tiene varias operaciones: indica `operationName`",
        )),
    }
}
