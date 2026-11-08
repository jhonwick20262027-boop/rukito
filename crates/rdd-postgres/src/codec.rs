//! Traducción entre el vocabulario neutro del puerto y los tipos de sqlx (§12.1).
//!
//! RF-187: solo se decodifican los tipos de PostgreSQL que tienen una variante
//! equivalente en `Value`, para que leer y volver a guardar sea simétrico.

use rdd_core::db::{Row, Value};
use rdd_core::error::{Error, Result};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::types::Oid;
use sqlx::postgres::{PgArgumentBuffer, PgArguments, PgRow, PgTypeInfo};
use sqlx::query::Query;
use sqlx::{Column as _, Encode, Postgres, Row as _, Type, TypeInfo as _, ValueRef as _};

/// Enlaza un valor neutro como parámetro (RF-113). Nada se interpola en el SQL.
pub fn bind<'q>(
    mut query: Query<'q, Postgres, PgArguments>,
    value: &Value,
) -> Query<'q, Postgres, PgArguments> {
    query = match value {
        Value::Null => query.bind(UntypedNull),
        Value::Bool(v) => query.bind(*v),
        Value::I64(v) => query.bind(*v),
        Value::F64(v) => query.bind(*v),
        Value::Text(v) => query.bind(v.clone()),
        Value::Bytes(v) => query.bind(v.clone()),
        Value::Uuid(v) => query.bind(*v),
        Value::Json(v) => query.bind(v.clone()),
    };
    query
}

/// `NULL` sin tipo declarado.
///
/// Ligarlo como `Option::<String>::None` enviaría el OID de `TEXT`: PostgreSQL
/// infiere la columna de un `NULL` sin tipo, pero rechaza uno ya tipado que no
/// coincide (`column is of type uuid but expression is of type text`). El OID 0
/// del mensaje Parse significa «sin especificar», así que lo infiere el servidor.
struct UntypedNull;

impl Type<Postgres> for UntypedNull {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }
}

impl Encode<'_, Postgres> for UntypedNull {
    fn encode_by_ref(
        &self,
        _buf: &mut PgArgumentBuffer,
    ) -> std::result::Result<IsNull, BoxDynError> {
        Ok(IsNull::Yes)
    }
}

/// Fila de sqlx → fila neutra, leyendo por nombre (§12.1: el orden no es contractual).
pub fn row(raw: &PgRow) -> Result<Row> {
    let mut out = Row::new();
    for (index, column) in raw.columns().iter().enumerate() {
        let name = column.name();
        out.insert(name, value(raw, index, name, column.type_info())?);
    }
    Ok(out)
}

fn value(raw: &PgRow, index: usize, column: &str, ty: &PgTypeInfo) -> Result<Value> {
    if raw
        .try_get_raw(index)
        .map_err(|err| decode_error(column, err))?
        .is_null()
    {
        return Ok(Value::Null);
    }

    Ok(match ty.name() {
        "BOOL" => Value::Bool(get(raw, index, column)?),
        "INT2" => Value::I64(i64::from(get::<i16>(raw, index, column)?)),
        "INT4" => Value::I64(i64::from(get::<i32>(raw, index, column)?)),
        "INT8" => Value::I64(get(raw, index, column)?),
        "FLOAT4" => Value::F64(f64::from(get::<f32>(raw, index, column)?)),
        "FLOAT8" => Value::F64(get(raw, index, column)?),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "UNKNOWN" => {
            Value::Text(get::<String>(raw, index, column)?)
        }
        "BYTEA" => Value::Bytes(get(raw, index, column)?),
        "UUID" => Value::Uuid(get(raw, index, column)?),
        "JSON" | "JSONB" => Value::Json(get(raw, index, column)?),
        other => {
            return Err(Error::internal(anyhow::anyhow!(
                "la columna `{column}` es de tipo {other}, que `Value` no puede representar \
                 (RF-187): convertirlo a texto se leería bien pero fallaría al volver a guardar. \
                 Usa un tipo con variante equivalente o deja la columna fuera del agregado (V15)"
            )));
        }
    })
}

fn get<T>(raw: &PgRow, index: usize, column: &str) -> Result<T>
where
    T: for<'r> sqlx::Decode<'r, Postgres> + Type<Postgres>,
{
    raw.try_get::<T, _>(index)
        .map_err(|err| decode_error(column, err))
}

fn decode_error(column: &str, err: sqlx::Error) -> Error {
    Error::internal(anyhow::anyhow!("columna `{column}`: {err}"))
}
