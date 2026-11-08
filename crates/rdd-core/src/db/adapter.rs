//! El puerto de persistencia (§12.1).
//!
//! Object-safe a propósito: `Repo<T>` guarda un `Arc<dyn DbAdapter>` y el motor se
//! elige al arrancar, no al compilar (RF-110, RF-111). El núcleo no conoce ningún
//! driver (RA-014); cada motor implementa este trait en su propio crate.

use std::sync::Arc;

use crate::db::{Row, Value};
use crate::error::Result;
use crate::http::BoxFuture;

/// Contrato mínimo que todo motor cumple.
///
/// Son **cuatro** operaciones, no seis: `find`/`get` se distinguen en `Repo<T>` y
/// `exists` se resuelve con `get`. Así cada adaptador porta menos superficie.
pub trait DbAdapter: Send + Sync + 'static {
    /// Nombre legible del motor, para el registro de arranque (RN-042).
    fn name(&self) -> &'static str;

    /// Una fila por identificador, o `None` si no existe.
    ///
    /// `id_column` es el nombre de la columna identificadora: un motor SQL la
    /// necesita para el `WHERE` y no puede adivinarla (§12.1). Los motores
    /// documentales y KV la ignoran.
    fn get<'a>(
        &'a self,
        table: &'a str,
        id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<Option<Row>>>;

    /// Todas las filas de la tabla. En Redis es un `SCAN` O(N) (RF-183).
    fn all<'a>(&'a self, table: &'a str) -> BoxFuture<'a, Result<Vec<Row>>>;

    /// Upsert atómico por identificador (RF-112).
    ///
    /// `columns[0]` y `values[0]` SON el identificador: es la convención que
    /// permite el upsert sin firmar un método por motor.
    fn save<'a>(
        &'a self,
        table: &'a str,
        columns: &'a [&'a str],
        values: &'a [Value],
    ) -> BoxFuture<'a, Result<()>>;

    /// Borra por identificador; `false` si no existía.
    fn delete<'a>(
        &'a self,
        table: &'a str,
        id_column: &'a str,
        id: &'a Value,
    ) -> BoxFuture<'a, Result<bool>>;
}

/// El adaptador activo, inyectable por tipo (RF-022, §12.1).
///
/// Lo registra `App::build()`; `Repo<T>` lo lee del contenedor, así que un
/// `#[service]` no necesita registrar nada por agregado.
#[derive(Clone)]
pub struct Db(pub Arc<dyn DbAdapter>);

impl Db {
    /// Nombre del motor activo.
    pub fn name(&self) -> &'static str {
        self.0.name()
    }
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Db").field(&self.0.name()).finish()
    }
}

impl From<Arc<dyn DbAdapter>> for Db {
    fn from(adapter: Arc<dyn DbAdapter>) -> Self {
        Self(adapter)
    }
}
