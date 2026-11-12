//! Ejecución de consultas, mutaciones y suscripciones (`RF-236`, `RF-238`).
//!
//! Solo los campos raíz son asíncronos y se ejecutan en una tarea para que un
//! `panic` se convierta en el error del campo y no tumbe la petición. La
//! serialización anidada es sincrónica: los objetos solo llevan datos.

use serde_json::{Map, Value};
use tokio::sync::mpsc;

use crate::schema::{GraphqlContext, GraphqlError, GraphqlStream, Schema, error_value};
use crate::types::{GraphqlValue, RootField, RootKind, SelectionSet};
use crate::validate::Prepared;

pub(crate) async fn execute(schema: &Schema, ctx: &GraphqlContext, prepared: Prepared) -> Value {
    let kind = prepared.kind.root_kind();
    let Some(root) = schema.root(kind) else {
        return error_value(vec![GraphqlError::message(format!(
            "el esquema no publica una raíz de {}",
            kind.keyword()
        ))]);
    };

    let mut data = Map::new();
    let mut errors = Vec::new();
    let mut data_is_null = false;

    for field in &prepared.selection.fields {
        if field.name == "__schema" || field.name == "__type" {
            data.insert(
                field.key.clone(),
                crate::introspection::resolve(schema, field),
            );
            continue;
        }
        let Some(def) = root.fields.iter().find(|item| item.name == field.name) else {
            errors.push(
                GraphqlError::message(format!("campo desconocido `{}`", field.name))
                    .with_path(vec![field.key.clone()]),
            );
            data_is_null = true;
            continue;
        };

        match resolve_root(ctx, def, &field.args).await {
            Ok(value) => {
                let serialized = value.serialize(&field.selection);
                if serialized.is_null() && def.ty.is_non_null() {
                    errors.push(
                        GraphqlError::message(format!(
                            "el campo `{}` es no-null y resolvió null",
                            field.key
                        ))
                        .with_path(vec![field.key.clone()]),
                    );
                    data_is_null = true;
                    break;
                }
                data.insert(field.key.clone(), serialized);
            }
            Err(err) => {
                errors.push(execution_error(err, &field.key));
                if def.ty.is_non_null() {
                    data_is_null = true;
                    break;
                }
                data.insert(field.key.clone(), Value::Null);
            }
        }
    }

    if !data_is_null {
        crate::types::serialize_typename(&mut data, &prepared.selection, kind.type_name());
    }

    let mut out = Map::new();
    out.insert(
        "data".to_owned(),
        if data_is_null {
            Value::Null
        } else {
            Value::Object(data)
        },
    );
    if !errors.is_empty() {
        out.insert(
            "errors".to_owned(),
            Value::Array(errors.iter().map(GraphqlError::to_json).collect()),
        );
    }
    Value::Object(out)
}

async fn resolve_root(
    ctx: &GraphqlContext,
    def: &RootField,
    args: &Value,
) -> rdd_core::Result<GraphqlValue> {
    let Some(resolve) = &def.resolve else {
        return Err(rdd_core::Error::internal(anyhow::anyhow!(
            "el campo `{}` no tiene resolutor",
            def.name
        )));
    };
    let ctx = ctx.clone();
    let args = args.clone();
    let future = resolve(&ctx, args);
    match tokio::spawn(future).await {
        Ok(result) => result,
        Err(join) => Err(rdd_core::Error::internal(anyhow::anyhow!(
            "el resolutor `{}` terminó sin respuesta: {join}",
            def.name
        ))),
    }
}

pub(crate) async fn subscribe(
    schema: &Schema,
    ctx: &GraphqlContext,
    prepared: Prepared,
) -> rdd_core::Result<GraphqlStream<Value>> {
    let Some(root) = schema.root(RootKind::Subscription) else {
        return Err(rdd_core::Error::bad_request(
            "el esquema no publica una raíz de suscripción",
        ));
    };
    let Some(field) = prepared.selection.fields.first() else {
        return Err(rdd_core::Error::bad_request(
            "la suscripción no selecciona ningún campo",
        ));
    };
    let Some(def) = root.fields.iter().find(|item| item.name == field.name) else {
        return Err(rdd_core::Error::bad_request(format!(
            "campo de suscripción desconocido `{}`",
            field.name
        )));
    };
    let Some(subscribe) = &def.subscribe else {
        return Err(rdd_core::Error::internal(anyhow::anyhow!(
            "el campo de suscripción `{}` no tiene resolutor",
            def.name
        )));
    };

    let start = {
        let ctx = ctx.clone();
        let args = field.args.clone();
        let future = subscribe(&ctx, args);
        tokio::spawn(future).await
    };

    let (tx, rx) = mpsc::channel(64);
    match start {
        Ok(Ok(mut stream)) => {
            let selection = field.selection.clone();
            let key = field.key.clone();
            let required = def.ty.is_non_null();
            let root_typenames = prepared.selection.typename_keys.clone();
            tokio::spawn(async move {
                while let Some(value) = stream.recv().await {
                    let payload =
                        subscription_payload(&value, &selection, &root_typenames, &key, required);
                    if tx.send(payload).await.is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Err(err)) => spawn_error_stream(tx, execution_error(err, &field.key)),
        Err(join) => {
            let err = rdd_core::Error::internal(anyhow::anyhow!(
                "el resolutor de `{}` terminó sin respuesta: {join}",
                def.name
            ));
            spawn_error_stream(tx, execution_error(err, &field.key));
        }
    }
    Ok(GraphqlStream::new(rx))
}

fn spawn_error_stream(tx: mpsc::Sender<Value>, error: GraphqlError) {
    tokio::spawn(async move {
        let payload = serde_json::json!({ "errors": [error.to_json()] });
        let _ = tx.send(payload).await;
    });
}

fn subscription_payload(
    value: &GraphqlValue,
    selection: &SelectionSet,
    root_typenames: &[String],
    key: &str,
    required: bool,
) -> Value {
    let serialized = value.serialize(selection);
    if serialized.is_null() && required {
        let error = GraphqlError::message(format!("el campo `{key}` es no-null y resolvió null"))
            .with_path(vec![key.to_owned()]);
        return serde_json::json!({ "errors": [error.to_json()] });
    }

    let mut data = Map::new();
    data.insert(key.to_owned(), serialized);
    for typename in root_typenames {
        data.insert(
            typename.clone(),
            Value::String(RootKind::Subscription.type_name().to_owned()),
        );
    }
    serde_json::json!({ "data": Value::Object(data) })
}

fn execution_error(err: rdd_core::Error, key: &str) -> GraphqlError {
    if err.is_internal() {
        tracing::error!(error = %err, field = key, "el resolutor GraphQL falló");
        GraphqlError::message("error interno").with_path(vec![key.to_owned()])
    } else {
        GraphqlError::message(err.client_detail()).with_path(vec![key.to_owned()])
    }
}
