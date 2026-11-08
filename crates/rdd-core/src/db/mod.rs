//! Persistencia multi-motor vía adaptadores (§12, ADR-16).
//!
//! El núcleo define el puerto ([`DbAdapter`]), el valor neutro ([`Value`]), la
//! fila neutra ([`Row`]) y el handle [`Repo<T>`]; cada motor vive en su propio
//! crate y se registra al arrancar (§12.6). Aquí no hay ningún driver (RA-014).

pub mod adapter;
pub mod memory;
pub mod persist;
pub mod registry;
pub mod repo;
pub mod row;
pub mod value;

pub use adapter::{Db, DbAdapter};
pub use memory::MemoryBackend;
pub use persist::Persist;
pub use registry::{AdapterFactory, declare, declared, factory_for, install, scheme_of};
pub use repo::Repo;
pub use row::Row;
pub use value::Value;

use std::sync::Arc;

use crate::error::Result;

/// Conecta el adaptador que corresponde al esquema de la URL (RF-111).
///
/// Es el camino que usan `App::connect()` y los crates de motor; un esquema sin
/// adaptador instalado falla aquí, antes de servir ninguna petición.
pub async fn connect(url: &str) -> Result<Arc<dyn DbAdapter>> {
    let factory = factory_for(url)?;
    factory(url.to_owned()).await
}
