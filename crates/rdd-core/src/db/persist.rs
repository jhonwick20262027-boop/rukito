//! `Persist`: el contrato neutro entre un agregado y el motor (§12.2).

use crate::db::{Row, Value};
use crate::domain::Aggregate;
use crate::error::Result;

/// Cómo se guarda y se lee un agregado, sin hablar SQL.
///
/// Lo genera `#[aggregate(table = ...)]` (RF-032) o el código de `src/generated/`
/// cuando se usa Prisma (§13.6). Los motores documentales y KV serializan el
/// agregado completo con `serde`, pero siguen necesitando `TABLE` y el id.
pub trait Persist: Aggregate + Sized {
    /// Tabla, colección o prefijo de clave.
    const TABLE: &'static str;

    /// Columnas en el mismo orden que `to_values`, con el identificador primero.
    const COLUMNS: &'static [&'static str];

    /// Valores del agregado, paralelos a `COLUMNS`.
    fn to_values(&self) -> Vec<Value>;

    /// Reconstruye el agregado desde una fila neutra.
    fn from_row(row: &Row) -> Result<Self>;
}
