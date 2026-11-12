//! Sistema de tipos del esquema y contrato que implementan las macros (§10.9).
//!
//! El esquema no tiene SDL: el tipo Rust es la definición (ADR-27). Las macros
//! generan [`GraphqlOutput`], [`GraphqlObjectType`] y [`GraphqlInput`], y el
//! motor solo ve metadatos más un serializador sincrónico. Los únicos puntos
//! asíncronos son los campos raíz, que viven en [`crate::RootField`].

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::GraphqlContext;

/// Referencia a un tipo del esquema, con lista y no-null (`RF-235`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeRef {
    /// Tipo con nombre (`String`, `Task`, `CreateTaskInput`…).
    Named(String),
    /// Lista; el elemento puede ser no-null.
    List(Box<TypeRef>),
    /// No-null. Es el modo por defecto de un campo Rust que no es `Option`.
    NonNull(Box<TypeRef>),
}

impl TypeRef {
    /// Tipo con nombre y nullable.
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    /// Tipo con nombre y no-null (`String!`).
    pub fn required(name: impl Into<String>) -> Self {
        Self::Named(name.into()).non_null()
    }

    /// Lista no-null de un tipo (`[T]!`).
    pub fn required_list(inner: TypeRef) -> Self {
        Self::List(Box::new(inner)).non_null()
    }

    /// Marca el tipo como no-null.
    pub fn non_null(self) -> Self {
        Self::NonNull(Box::new(self))
    }

    /// Quita el `NonNull` más externo: es lo que hace `Option<T>`.
    pub fn nullable(self) -> Self {
        match self {
            Self::NonNull(inner) => *inner,
            other => other,
        }
    }

    /// Cierto si el tipo es no-null en su nivel externo.
    pub fn is_non_null(&self) -> bool {
        matches!(self, Self::NonNull(_))
    }

    /// Nombre del tipo con nombre que hay debajo de listas y `NonNull`.
    pub fn named_type(&self) -> &str {
        match self {
            Self::Named(name) => name,
            Self::List(inner) | Self::NonNull(inner) => inner.named_type(),
        }
    }

    /// Representación GraphQL (`[ID!]!`), para mensajes e introspección.
    pub fn signature(&self) -> String {
        match self {
            Self::Named(name) => name.clone(),
            Self::List(inner) => format!("[{}]", inner.signature()),
            Self::NonNull(inner) => format!("{}!", inner.signature()),
        }
    }
}

impl std::fmt::Display for TypeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.signature())
    }
}

/// Argumento de un campo raíz (`RF-240`).
#[derive(Clone, Debug, PartialEq)]
pub struct ArgDef {
    /// Nombre publicado, en `camelCase`.
    pub name: &'static str,
    /// Doc comment del parámetro, si lo hay.
    pub description: Option<&'static str>,
    /// Tipo de entrada.
    pub ty: TypeRef,
}

/// Campo de un tipo objeto (`RF-237`).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    /// Nombre publicado, en `camelCase`.
    pub name: &'static str,
    /// Doc comment del campo, si lo hay.
    pub description: Option<&'static str>,
    /// Tipo de salida.
    pub ty: TypeRef,
    /// Argumentos; solo los campos raíz tienen.
    pub args: Vec<ArgDef>,
}

/// Campo de un tipo de entrada (`RF-237`).
#[derive(Clone, Debug, PartialEq)]
pub struct InputFieldDef {
    /// Nombre publicado, en `camelCase`.
    pub name: &'static str,
    /// Doc comment del campo, si lo hay.
    pub description: Option<&'static str>,
    /// Tipo de entrada.
    pub ty: TypeRef,
    /// Valor por defecto, si la macro lo declaró.
    pub default: Option<Value>,
}

/// Tipo objeto registrado por una macro.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectTypeDef {
    /// Nombre publicado.
    pub name: &'static str,
    /// Doc comment del tipo.
    pub description: Option<&'static str>,
    /// Campos publicados, en orden de declaración.
    pub fields: Vec<FieldDef>,
}

/// Tipo de entrada registrado por una macro.
#[derive(Clone, Debug, PartialEq)]
pub struct InputObjectDef {
    /// Nombre publicado.
    pub name: &'static str,
    /// Doc comment del tipo.
    pub description: Option<&'static str>,
    /// Campos publicados.
    pub fields: Vec<InputFieldDef>,
}

/// Colección de tipos que se recorre desde las raíces al construir el esquema.
///
/// Las implementaciones derivadas se registran una sola vez: el conjunto
/// `seen_*` corta los ciclos entre tipos que se referencian mutuamente.
#[derive(Default)]
pub struct TypeRegistry {
    /// Objetos encontrados.
    pub objects: Vec<ObjectTypeDef>,
    /// Entradas encontradas.
    pub inputs: Vec<InputObjectDef>,
    /// Nombres ya visitados.
    seen_objects: HashSet<&'static str>,
    /// Nombres de entrada ya visitados.
    seen_inputs: HashSet<&'static str>,
    /// Tipos declarados dos veces con una forma distinta (`RF-244`).
    pub conflicts: Vec<String>,
}

impl TypeRegistry {
    /// Registra un objeto y recorre sus campos.
    pub fn object(
        &mut self,
        name: &'static str,
        description: Option<&'static str>,
        fields: impl FnOnce() -> Vec<FieldDef>,
        visit: impl FnOnce(&mut TypeRegistry),
    ) {
        if self.seen_objects.insert(name) {
            self.objects.push(ObjectTypeDef {
                name,
                description,
                fields: fields(),
            });
            visit(self);
        } else if let Some(existing) = self.objects.iter().find(|item| item.name == name)
            && existing.fields != fields()
        {
            self.conflicts.push(format!(
                "el tipo `{name}` se define dos veces con campos distintos"
            ));
        }
    }

    /// Registra un tipo de entrada y recorre sus campos.
    pub fn input(
        &mut self,
        name: &'static str,
        description: Option<&'static str>,
        fields: impl FnOnce() -> Vec<InputFieldDef>,
        visit: impl FnOnce(&mut TypeRegistry),
    ) {
        if self.seen_inputs.insert(name) {
            self.inputs.push(InputObjectDef {
                name,
                description,
                fields: fields(),
            });
            visit(self);
        } else if let Some(existing) = self.inputs.iter().find(|item| item.name == name)
            && existing.fields != fields()
        {
            self.conflicts.push(format!(
                "el tipo de entrada `{name}` se define dos veces con campos distintos"
            ));
        }
    }
}

/// Un campo tal como lo pidió el documento, ya expandido y con sus argumentos.
#[derive(Clone, Debug, Default)]
pub struct SelectionSet {
    /// Campos en el orden del documento.
    pub fields: Vec<SelectedField>,
    /// Alias de `__typename`, que se resuelve sin tocar al objeto.
    pub typename_keys: Vec<String>,
}

/// Un campo seleccionado, con alias y subselección ya expandida.
#[derive(Clone, Debug)]
pub struct SelectedField {
    /// Clave de la respuesta (alias o nombre).
    pub key: String,
    /// Nombre del campo en el esquema.
    pub name: String,
    /// Argumentos ya coercidos (objeto JSON; vacío si no tiene).
    pub args: Value,
    /// Subselección; vacía en escalares.
    pub selection: SelectionSet,
}

impl SelectedField {
    /// Argumento ya coercido por nombre.
    pub fn arg(&self, name: &str) -> Value {
        self.args.get(name).cloned().unwrap_or(Value::Null)
    }
}

/// Inserta las claves de `__typename` que pidió la selección (`RF-237`).
pub fn serialize_typename(map: &mut Map<String, Value>, selection: &SelectionSet, name: &str) {
    for key in &selection.typename_keys {
        map.insert(key.clone(), Value::String(name.to_owned()));
    }
}

/// Salida ya resuelta de un campo raíz o de un elemento de suscripción.
///
/// Captura el valor Rust y lo serializa con la selección que llegue: así el
/// resolutor no conoce la forma de la consulta.
pub struct GraphqlValue(Box<dyn Fn(&SelectionSet) -> Value + Send + Sync>);

impl GraphqlValue {
    /// Envuelve un valor serializable por el motor.
    pub fn new<T: GraphqlOutput + Send + 'static>(value: T) -> Self {
        Self(Box::new(move |selection| value.serialize(selection)))
    }

    /// Valor nulo, útil cuando un resolutor opcional no encuentra nada.
    pub fn null() -> Self {
        Self(Box::new(|_| Value::Null))
    }

    /// Serializa con la selección del campo.
    pub fn serialize(&self, selection: &SelectionSet) -> Value {
        (self.0)(selection)
    }
}

impl std::fmt::Debug for GraphqlValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GraphqlValue")
    }
}

/// Tipo de salida del esquema (`RF-235`, `RF-240`).
///
/// La implementan los escalares, `Option`/`Vec` y los tipos con
/// `#[derive(GraphqlObject)]`. La serialización es sincrónica: los resolutores
/// asíncronos solo existen en la raíz.
pub trait GraphqlOutput: Send + Sync + 'static {
    /// Tipo publicado.
    fn type_ref() -> TypeRef;

    /// Registra los tipos objeto que aparecen dentro de este.
    fn collect_types(_types: &mut TypeRegistry) {}

    /// Serializa el valor con la subselección del campo.
    fn serialize(&self, selection: &SelectionSet) -> Value;
}

/// Tipo objeto con nombre, que puede recibir una selección (`RF-237`).
pub trait GraphqlObjectType: GraphqlOutput {
    /// Nombre publicado.
    fn object_name() -> &'static str;

    /// Doc comment del tipo.
    fn object_description() -> Option<&'static str>;

    /// Campos publicados.
    fn object_fields() -> Vec<FieldDef>;
}

/// Tipo de entrada del esquema (`RF-235`, `RF-240`).
pub trait GraphqlInput: Sized + Send + 'static {
    /// Tipo publicado.
    fn type_ref() -> TypeRef;

    /// Registra los tipos de entrada que aparecen dentro de este.
    fn collect_types(_types: &mut TypeRegistry) {}

    /// Lee el valor ya validado desde el JSON de la petición.
    fn from_json(value: &Value) -> Result<Self, String>;
}

macro_rules! scalar {
    ($ty:ty, $name:literal, $parse:expr, $out:expr) => {
        impl GraphqlOutput for $ty {
            fn type_ref() -> TypeRef {
                TypeRef::required($name)
            }

            fn serialize(&self, _selection: &SelectionSet) -> Value {
                $out(self)
            }
        }

        impl GraphqlInput for $ty {
            fn type_ref() -> TypeRef {
                TypeRef::required($name)
            }

            fn from_json(value: &Value) -> Result<Self, String> {
                #[allow(clippy::redundant_closure_call)]
                ($parse)(value)
            }
        }
    };
}

scalar!(
    String,
    "String",
    |value: &Value| value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "se esperaba una cadena".to_owned()),
    |value: &String| Value::String(value.clone())
);

scalar!(
    bool,
    "Boolean",
    |value: &Value| value
        .as_bool()
        .ok_or_else(|| "se esperaba un booleano".to_owned()),
    |value: &bool| Value::Bool(*value)
);

scalar!(
    i32,
    "Int",
    |value: &Value| {
        let number = value
            .as_i64()
            .ok_or_else(|| "se esperaba un entero".to_owned())?;
        i32::try_from(number).map_err(|_| "el entero está fuera del rango de Int".to_owned())
    },
    |value: &i32| Value::Number((*value).into())
);

scalar!(
    f64,
    "Float",
    |value: &Value| value
        .as_f64()
        .ok_or_else(|| "se esperaba un número".to_owned()),
    |value: &f64| serde_json::Number::from_f64(*value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
);

scalar!(
    uuid::Uuid,
    "ID",
    |value: &Value| match value {
        Value::String(text) =>
            uuid::Uuid::parse_str(text).map_err(|_| format!("`{text}` no es un ID válido")),
        _ => Err("se esperaba un ID en texto".to_owned()),
    },
    |value: &uuid::Uuid| Value::String(value.to_string())
);

impl<T: GraphqlOutput> GraphqlOutput for Option<T> {
    fn type_ref() -> TypeRef {
        T::type_ref().nullable()
    }

    fn collect_types(types: &mut TypeRegistry) {
        T::collect_types(types);
    }

    fn serialize(&self, selection: &SelectionSet) -> Value {
        match self {
            Some(value) => value.serialize(selection),
            None => Value::Null,
        }
    }
}

impl<T: GraphqlInput> GraphqlInput for Option<T> {
    fn type_ref() -> TypeRef {
        T::type_ref().nullable()
    }

    fn collect_types(types: &mut TypeRegistry) {
        T::collect_types(types);
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        if value.is_null() {
            Ok(None)
        } else {
            T::from_json(value).map(Some)
        }
    }
}

impl<T: GraphqlOutput> GraphqlOutput for Vec<T> {
    fn type_ref() -> TypeRef {
        TypeRef::required_list(T::type_ref())
    }

    fn collect_types(types: &mut TypeRegistry) {
        T::collect_types(types);
    }

    fn serialize(&self, selection: &SelectionSet) -> Value {
        Value::Array(self.iter().map(|item| item.serialize(selection)).collect())
    }
}

impl<T: GraphqlInput> GraphqlInput for Vec<T> {
    fn type_ref() -> TypeRef {
        TypeRef::required_list(T::type_ref())
    }

    fn collect_types(types: &mut TypeRegistry) {
        T::collect_types(types);
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let items = value
            .as_array()
            .ok_or_else(|| "se esperaba una lista".to_owned())?;
        items.iter().map(T::from_json).collect()
    }
}

/// Argumento ya validado, extraído en el resolutor generado por la macro.
///
/// Un fallo aquí solo puede venir de un desajuste entre validación y macro: se
/// reporta como error interno y nunca se expone el detalle (`RF-244`).
pub fn argument<T: GraphqlInput>(args: &Value, name: &str) -> rdd_core::Result<T> {
    let value = args.get(name).cloned().unwrap_or(Value::Null);
    T::from_json(&value).map_err(|err| {
        rdd_core::Error::internal(anyhow::anyhow!(
            "el argumento `{name}` no se pudo leer tras la validación: {err}"
        ))
    })
}

/// Construye el valor de retorno de un campo raíz.
pub fn resolved<T: GraphqlOutput + Send + 'static>(value: T) -> GraphqlValue {
    GraphqlValue::new(value)
}

/// Firma del resolutor de un campo raíz de consulta o mutación.
pub type ResolveFn = Arc<
    dyn Fn(
            &GraphqlContext,
            Value,
        ) -> rdd_core::http::BoxFuture<'static, rdd_core::Result<GraphqlValue>>
        + Send
        + Sync,
>;

/// Firma del resolutor de un campo de suscripción.
pub type SubscribeFn = Arc<
    dyn Fn(
            &GraphqlContext,
            Value,
        ) -> rdd_core::http::BoxFuture<
            'static,
            rdd_core::Result<crate::GraphqlStream<GraphqlValue>>,
        > + Send
        + Sync,
>;

/// Raíz del esquema a la que pertenece un campo (`RF-240`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    /// `query`.
    Query,
    /// `mutation`.
    Mutation,
    /// `subscription`.
    Subscription,
}

impl RootKind {
    /// Nombre del tipo raíz publicado en la introspección.
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::Mutation => "Mutation",
            Self::Subscription => "Subscription",
        }
    }

    /// `query`, `mutation` o `subscription`.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Mutation => "mutation",
            Self::Subscription => "subscription",
        }
    }
}

/// Campo raíz con su metadato y su resolutor (`RF-240`).
#[derive(Clone)]
pub struct RootField {
    /// Nombre publicado.
    pub name: &'static str,
    /// Doc comment del método.
    pub description: Option<&'static str>,
    /// Argumentos publicados.
    pub args: Vec<ArgDef>,
    /// Tipo de salida.
    pub ty: TypeRef,
    /// Raíz a la que pertenece.
    pub kind: RootKind,
    /// Resolutor de consulta o mutación.
    pub resolve: Option<ResolveFn>,
    /// Resolutor de suscripción.
    pub subscribe: Option<SubscribeFn>,
}

impl std::fmt::Debug for RootField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootField")
            .field("name", &self.name)
            .field("ty", &self.ty)
            .field("kind", &self.kind)
            .finish()
    }
}

/// Las raíces que aporta un `impl` marcado con `#[graphql_object(...)]`.
pub trait GraphqlRoot: Default + Send + Sync + 'static {
    /// Campos raíz, en orden de declaración.
    fn root_fields() -> Vec<RootField>;

    /// Registra los tipos de argumentos y de retorno.
    fn collect_root_types(types: &mut TypeRegistry);
}
