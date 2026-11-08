//! El agregado de sonda: una columna por variante de `Value` (§12.4).

use rdd_core::db::{Persist, Row, Value};
use rdd_core::domain::Aggregate;
use rdd_core::error::Result;
use uuid::Uuid;

/// Tabla que la suite espera encontrar **vacía** en el motor bajo prueba.
///
/// La suite no crea tablas: el puerto no tiene DDL y `.run()` no aplica
/// migraciones (RF-117). Cada crate de motor crea esta tabla en su propio test de
/// integración con su cliente nativo, usando [`COLUMNS`] como contrato.
///
/// DDL de referencia para PostgreSQL:
///
/// ```sql
/// CREATE TABLE rdd_db_probe (
///     id     UUID PRIMARY KEY,
///     flag   BOOLEAN NOT NULL,
///     count  BIGINT NOT NULL,
///     ratio  DOUBLE PRECISION NOT NULL,
///     label  TEXT NOT NULL,
///     blob   BYTEA NOT NULL,
///     doc    JSONB NOT NULL,
///     maybe  TEXT
/// );
/// ```
pub const TABLE: &str = "rdd_db_probe";

/// Columnas en el orden de `to_values`, con el identificador primero (§12.2).
pub const COLUMNS: &[&str] = &[
    "id", "flag", "count", "ratio", "label", "blob", "doc", "maybe",
];

/// Una fila que ejercita las ocho variantes de `Value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// `Value::Uuid`
    pub id: Uuid,
    /// `Value::Bool`
    pub flag: bool,
    /// `Value::I64`
    pub count: i64,
    /// `Value::F64`
    pub ratio: f64,
    /// `Value::Text`
    pub label: String,
    /// `Value::Bytes`
    pub blob: Vec<u8>,
    /// `Value::Json`
    pub doc: serde_json::Value,
    /// `Value::Null` cuando es `None`.
    pub maybe: Option<String>,
}

impl Probe {
    /// Fila de prueba con un id nuevo y valores exactamente representables.
    ///
    /// `ratio` es 1.5 a propósito: un decimal no representable haría que el
    /// round-trip fallara por la coma flotante del motor, no por el adaptador.
    pub fn sample(label: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            flag: true,
            count: -42,
            ratio: 1.5,
            label: label.to_owned(),
            blob: vec![0, 1, 2, 255],
            doc: serde_json::json!({ "a": [1, 2], "b": "texto" }),
            maybe: None,
        }
    }
}

impl Aggregate for Probe {
    type Id = Uuid;

    fn id(&self) -> &Self::Id {
        &self.id
    }
}

impl Persist for Probe {
    const TABLE: &'static str = TABLE;
    const COLUMNS: &'static [&'static str] = COLUMNS;

    fn to_values(&self) -> Vec<Value> {
        vec![
            Value::Uuid(self.id),
            Value::Bool(self.flag),
            Value::I64(self.count),
            Value::F64(self.ratio),
            Value::from(self.label.clone()),
            Value::Bytes(self.blob.clone()),
            Value::Json(self.doc.clone()),
            Value::from(self.maybe.clone()),
        ]
    }

    fn from_row(row: &Row) -> Result<Self> {
        let maybe = row.value("maybe")?;
        Ok(Self {
            id: row.value("id")?.as_uuid()?,
            flag: row.value("flag")?.as_bool()?,
            count: row.value("count")?.as_i64()?,
            ratio: row.value("ratio")?.as_f64()?,
            label: row.value("label")?.as_text()?.to_owned(),
            blob: row.value("blob")?.as_bytes()?.to_vec(),
            doc: row.value("doc")?.as_json()?.clone(),
            maybe: if maybe.is_null() {
                None
            } else {
                Some(maybe.as_text()?.to_owned())
            },
        })
    }
}
