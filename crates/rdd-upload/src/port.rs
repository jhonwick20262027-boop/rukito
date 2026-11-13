//! Puerto de storage de objetos (§10.13, RF-269).
//!
//! Object-safe: los adaptadores se guardan como `Arc<dyn ObjectStorage>` en el
//! contenedor de la app, que los usa desde sus services.

use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;

use crate::key::ObjectKey;
use crate::meta::{ObjectMeta, PutOptions, StoredObject};

/// Futuro boxeado que devuelve el puerto, independiente del runtime.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Operaciones mínimas de un storage de objetos.
///
/// `get` y `head` devuelven `None` cuando la clave no existe; `delete` es
/// idempotente. Esa semántica es común a `MemoryStorage` y a los adaptadores.
pub trait ObjectStorage: Send + Sync + 'static {
    /// Guarda o sobrescribe el objeto.
    fn put(
        &self,
        key: &ObjectKey,
        body: Bytes,
        options: PutOptions,
    ) -> BoxFuture<'_, rdd_core::Result<ObjectMeta>>;

    /// Lee el objeto completo; `None` si no existe.
    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<StoredObject>>>;

    /// Lee solo la metadata; `None` si no existe.
    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<ObjectMeta>>>;

    /// Borra el objeto; un 404 del proveedor no es error.
    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<()>>;
}
