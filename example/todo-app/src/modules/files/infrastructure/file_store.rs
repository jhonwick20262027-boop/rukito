//! Adaptador del puerto de storage para el contenedor (§26.13, RA-007).
//!
//! El módulo recibe un `Arc<dyn ObjectStorage>` ya construido por
//! `config/upload_config.rs`; este envoltorio lo deja resolver desde el
//! contenedor sin que la capa de aplicación nombre el adaptador concreto.

use std::sync::Arc;

use rdd::prelude::*;
use rdd::upload::{BoxFuture, ObjectKey, ObjectMeta, ObjectStorage, PutOptions, StoredObject};

pub struct FileStore {
    inner: Arc<dyn ObjectStorage>,
}

impl FileStore {
    pub fn new(inner: Arc<dyn ObjectStorage>) -> Self {
        Self { inner }
    }
}

impl ObjectStorage for FileStore {
    fn put(
        &self,
        key: &ObjectKey,
        body: Bytes,
        options: PutOptions,
    ) -> BoxFuture<'_, Result<ObjectMeta>> {
        self.inner.put(key, body, options)
    }

    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, Result<Option<StoredObject>>> {
        self.inner.get(key)
    }

    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, Result<Option<ObjectMeta>>> {
        self.inner.head(key)
    }

    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, Result<()>> {
        self.inner.delete(key)
    }
}
