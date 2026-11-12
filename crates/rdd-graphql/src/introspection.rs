//! Introspección del esquema (`RF-237`).
//!
//! Se construye el árbol completo de `__schema` y `__type` y luego se recorta
//! con la selección que pidió el cliente, incluyendo alias y `__typename`.

use serde_json::{Map, Value, json};

use crate::schema::{SCALARS, Schema};
use crate::types::{FieldDef, InputObjectDef, ObjectTypeDef, SelectedField, SelectionSet, TypeRef};

pub(crate) fn resolve(schema: &Schema, field: &SelectedField) -> Value {
    match field.name.as_str() {
        "__schema" => select_json(&schema_json(schema), &field.selection),
        "__type" => {
            let Some(name) = field.args.get("name").and_then(Value::as_str) else {
                return Value::Null;
            };
            match type_json(schema, name) {
                Some(value) => select_json(&value, &field.selection),
                None => Value::Null,
            }
        }
        _ => Value::Null,
    }
}

fn schema_json(schema: &Schema) -> Value {
    let root = |name: &str| type_json(schema, name).unwrap_or(Value::Null);
    json!({
        "__typename": "__Schema",
        "description": Value::Null,
        "queryType": root("Query"),
        "mutationType": root("Mutation"),
        "subscriptionType": root("Subscription"),
        "types": schema
            .type_names()
            .iter()
            .filter_map(|name| type_json(schema, name))
            .collect::<Vec<_>>(),
        "directives": [],
    })
}

fn type_json(schema: &Schema, name: &str) -> Option<Value> {
    if SCALARS.contains(&name) {
        return Some(scalar_json(name));
    }
    if let Some(input) = schema.input(name) {
        return Some(input_object_json(schema, input));
    }
    schema
        .object(name)
        .map(|object| object_json(schema, object))
}

fn scalar_json(name: &str) -> Value {
    json!({
        "__typename": "__Type",
        "kind": "SCALAR",
        "name": name,
        "description": Value::Null,
        "fields": Value::Null,
        "inputFields": Value::Null,
        "interfaces": Value::Null,
        "enumValues": Value::Null,
        "possibleTypes": Value::Null,
        "ofType": Value::Null,
    })
}

fn object_json(schema: &Schema, object: &ObjectTypeDef) -> Value {
    json!({
        "__typename": "__Type",
        "kind": "OBJECT",
        "name": object.name,
        "description": description(object.description),
        "fields": object
            .fields
            .iter()
            .map(|field| field_json(schema, field))
            .collect::<Vec<_>>(),
        "inputFields": Value::Null,
        "interfaces": Value::Null,
        "enumValues": Value::Null,
        "possibleTypes": Value::Null,
        "ofType": Value::Null,
    })
}

fn field_json(schema: &Schema, field: &FieldDef) -> Value {
    json!({
        "__typename": "__Field",
        "name": field.name,
        "description": description(field.description),
        "args": field
            .args
            .iter()
            .map(|arg| input_value_json(schema, arg.name, arg.description, &arg.ty, None))
            .collect::<Vec<_>>(),
        "type": type_ref_json(schema, &field.ty),
        "isDeprecated": false,
        "deprecationReason": Value::Null,
    })
}

fn input_object_json(schema: &Schema, input: &InputObjectDef) -> Value {
    json!({
        "__typename": "__Type",
        "kind": "INPUT_OBJECT",
        "name": input.name,
        "description": description(input.description),
        "fields": Value::Null,
        "inputFields": input
            .fields
            .iter()
            .map(|field| {
                input_value_json(
                    schema,
                    field.name,
                    field.description,
                    &field.ty,
                    field.default.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        "interfaces": Value::Null,
        "enumValues": Value::Null,
        "possibleTypes": Value::Null,
        "ofType": Value::Null,
    })
}

fn input_value_json(
    schema: &Schema,
    name: &str,
    text: Option<&'static str>,
    ty: &TypeRef,
    default: Option<&Value>,
) -> Value {
    let default_value = default
        .and_then(|value| serde_json::to_string(value).ok())
        .map(Value::String)
        .unwrap_or(Value::Null);
    json!({
        "__typename": "__InputValue",
        "name": name,
        "description": description(text),
        "type": type_ref_json(schema, ty),
        "defaultValue": default_value,
    })
}

fn type_ref_json(schema: &Schema, ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Named(name) => named_ref_json(schema, name),
        TypeRef::List(inner) => wrapper_json("LIST", type_ref_json(schema, inner)),
        TypeRef::NonNull(inner) => wrapper_json("NON_NULL", type_ref_json(schema, inner)),
    }
}

fn named_ref_json(schema: &Schema, name: &str) -> Value {
    let kind = if SCALARS.contains(&name) {
        "SCALAR"
    } else if schema.input(name).is_some() {
        "INPUT_OBJECT"
    } else if schema.object(name).is_some() {
        "OBJECT"
    } else {
        "SCALAR"
    };
    json!({
        "__typename": "__Type",
        "kind": kind,
        "name": name,
        "description": Value::Null,
        "fields": Value::Null,
        "inputFields": Value::Null,
        "interfaces": Value::Null,
        "enumValues": Value::Null,
        "possibleTypes": Value::Null,
        "ofType": Value::Null,
    })
}

fn wrapper_json(kind: &str, inner: Value) -> Value {
    json!({
        "__typename": "__Type",
        "kind": kind,
        "name": Value::Null,
        "description": Value::Null,
        "fields": Value::Null,
        "inputFields": Value::Null,
        "interfaces": Value::Null,
        "enumValues": Value::Null,
        "possibleTypes": Value::Null,
        "ofType": inner,
    })
}

fn description(text: Option<&'static str>) -> Value {
    text.map(|text| Value::String(text.to_owned()))
        .unwrap_or(Value::Null)
}

fn select_json(value: &Value, selection: &SelectionSet) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| select_json(item, selection))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = Map::new();
            for field in &selection.fields {
                let selected = match map.get(&field.name) {
                    Some(inner) => select_json(inner, &field.selection),
                    None => Value::Null,
                };
                out.insert(field.key.clone(), selected);
            }
            for key in &selection.typename_keys {
                out.insert(
                    key.clone(),
                    map.get("__typename").cloned().unwrap_or(Value::Null),
                );
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}
