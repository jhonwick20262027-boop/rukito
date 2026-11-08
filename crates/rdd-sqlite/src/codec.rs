//! Conversión entre `Value` y los tipos declarados de SQLite.
//!
//! SQLx expone solo afinidades estándar; se consulta `pragma_table_info` para
//! recuperar los tipos declarados de UUID y JSON, que SQLite acepta pero SQLx
//! no representa como afinidades propias.

use std::collections::HashMap;

use rdd_core::db::{Row, Value};
use rdd_core::error::{Error, Result};
use sqlx::query::Query;
use sqlx::sqlite::{Sqlite, SqliteArguments, SqliteRow, SqliteTypeInfo};
use sqlx::{Column as _, Row as _, TypeInfo as _, ValueRef as _};

/// Enlaza valores como parámetros; UUID y JSON se almacenan en columnas
/// declaradas `UUID` y `JSON`, para conservar su variante al leerlos.
pub fn bind<'q>(
    mut query: Query<'q, Sqlite, SqliteArguments>,
    value: &Value,
) -> Query<'q, Sqlite, SqliteArguments> {
    query = match value {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(value) => query.bind(i64::from(*value)),
        Value::I64(value) => query.bind(*value),
        Value::F64(value) => query.bind(*value),
        Value::Text(value) => query.bind(value.clone()),
        Value::Bytes(value) => query.bind(value.clone()),
        Value::Uuid(value) => query.bind(value.to_string()),
        Value::Json(value) => query.bind(value.to_string()),
    };
    query
}

/// Convierte una fila SQLx en la fila independiente del motor.
pub fn row(raw: &SqliteRow, column_types: &HashMap<String, String>) -> Result<Row> {
    let mut out = Row::new();
    for (index, column) in raw.columns().iter().enumerate() {
        let name = column.name();
        let value = value(
            raw,
            index,
            name,
            column_types.get(name).map(String::as_str),
            column.type_info(),
        )?;
        out.insert(name, value);
    }
    Ok(out)
}

fn value(
    raw: &SqliteRow,
    index: usize,
    column: &str,
    declared_type: Option<&str>,
    ty: &SqliteTypeInfo,
) -> Result<Value> {
    if raw
        .try_get_raw(index)
        .map_err(|err| decode_error(column, err))?
        .is_null()
    {
        return Ok(Value::Null);
    }

    let type_name = declared_type
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| ty.name())
        .to_ascii_uppercase();
    match type_name.as_str() {
        "BOOL" | "BOOLEAN" => get::<i64>(raw, index, column).map(|value| Value::Bool(value != 0)),
        "UUID" => {
            let text = get::<String>(raw, index, column)?;
            uuid::Uuid::parse_str(&text)
                .map(Value::Uuid)
                .map_err(|err| {
                    Error::internal(anyhow::anyhow!(
                        "columna `{column}` contiene un UUID inválido: {err}"
                    ))
                })
        }
        "JSON" | "JSONB" => {
            let text = get::<String>(raw, index, column)?;
            serde_json::from_str(&text).map(Value::Json).map_err(|err| {
                Error::internal(anyhow::anyhow!(
                    "columna `{column}` contiene JSON inválido: {err}"
                ))
            })
        }
        "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" => {
            get(raw, index, column).map(Value::I64)
        }
        "REAL" | "FLOAT" | "DOUBLE" | "DOUBLE PRECISION" => get(raw, index, column).map(Value::F64),
        "TEXT" | "VARCHAR" | "CHAR" | "CLOB" => get(raw, index, column).map(Value::Text),
        "BLOB" => get(raw, index, column).map(Value::Bytes),
        other => Err(Error::internal(anyhow::anyhow!(
            "la columna `{column}` es de tipo SQLite {other}, que `Value` no puede representar \
             simétricamente. Declara columnas UUID como UUID y JSON como JSON."
        ))),
    }
}

fn get<T>(raw: &SqliteRow, index: usize, column: &str) -> Result<T>
where
    for<'r> T: sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite>,
{
    raw.try_get::<T, _>(index)
        .map_err(|err| decode_error(column, err))
}

fn decode_error(column: &str, err: sqlx::Error) -> Error {
    Error::internal(anyhow::anyhow!("columna `{column}`: {err}"))
}
