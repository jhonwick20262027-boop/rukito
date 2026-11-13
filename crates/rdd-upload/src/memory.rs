//! `MemoryStorage`: el puerto en un mapa, para pruebas y desarrollo (§10.13).
//!
//! Comparte la semántica de los adaptadores: `get`/`head` devuelven `None` si la
//! clave no existe, `delete` es idempotente y el `etag` es opaco (RF-271).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::anyhow;
use bytes::Bytes;

use crate::key::ObjectKey;
use crate::meta::{ObjectMeta, PutOptions, StoredObject};
use crate::port::{BoxFuture, ObjectStorage};

/// Límite por defecto del puerto (64 MiB).
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;

/// Storage en memoria con límite de objeto.
#[derive(Debug)]
pub struct MemoryStorage {
    max_bytes: usize,
    sequence: AtomicU64,
    objects: Mutex<BTreeMap<String, StoredObject>>,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorage {
    /// Storage vacío con el límite por defecto.
    pub fn new() -> Self {
        Self::with_max_bytes(DEFAULT_MAX_OBJECT_BYTES)
    }

    /// Storage vacío con un límite propio.
    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            sequence: AtomicU64::new(0),
            objects: Mutex::new(BTreeMap::new()),
        }
    }

    /// Límite configurado.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    fn reject_oversize(&self, size: usize) -> rdd_core::Result<()> {
        if size > self.max_bytes {
            return Err(rdd_core::Error::internal(anyhow!(
                "el objeto mide {size} bytes y el límite es {} (RF-271)",
                self.max_bytes
            )));
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, StoredObject>> {
        // Un pánico anterior no puede dejar el mapa inconsistentemente prestado:
        // solo se toca dentro de este módulo y sin await.
        self.objects
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ObjectStorage for MemoryStorage {
    fn put(
        &self,
        key: &ObjectKey,
        body: Bytes,
        options: PutOptions,
    ) -> BoxFuture<'_, rdd_core::Result<ObjectMeta>> {
        let key = key.clone();
        Box::pin(async move {
            self.reject_oversize(body.len())?;
            let etag = format!("mem-{}", self.sequence.fetch_add(1, Ordering::Relaxed) + 1);
            let meta = ObjectMeta {
                key: key.clone(),
                size: body.len() as u64,
                content_type: options.content_type.clone(),
                etag: Some(etag),
                last_modified: Some(SystemTime::now()),
            };
            self.lock().insert(
                key.as_str().to_owned(),
                StoredObject {
                    meta: meta.clone(),
                    body,
                },
            );
            Ok(meta)
        })
    }

    fn get(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<StoredObject>>> {
        let key = key.clone();
        Box::pin(async move { Ok(self.lock().get(key.as_str()).cloned()) })
    }

    fn head(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<Option<ObjectMeta>>> {
        let key = key.clone();
        Box::pin(async move {
            Ok(self
                .lock()
                .get(key.as_str())
                .map(|object| object.meta.clone()))
        })
    }

    fn delete(&self, key: &ObjectKey) -> BoxFuture<'_, rdd_core::Result<()>> {
        let key = key.clone();
        Box::pin(async move {
            self.lock().remove(key.as_str());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(raw: &str) -> ObjectKey {
        ObjectKey::parse(raw).unwrap()
    }

    #[tokio::test]
    async fn put_get_head_y_delete_comparten_semantica() {
        let storage = MemoryStorage::new();
        let k = key("files/a/uno.txt");
        let meta = storage
            .put(
                &k,
                Bytes::from_static(b"hola"),
                PutOptions::new()
                    .content_type("text/plain")
                    .metadata("origen", "test"),
            )
            .await
            .unwrap();
        assert_eq!(meta.size, 4);
        assert_eq!(meta.content_type.as_deref(), Some("text/plain"));

        let object = storage.get(&k).await.unwrap().unwrap();
        assert_eq!(object.body, Bytes::from_static(b"hola"));
        assert_eq!(object.meta.etag, meta.etag);
        assert_eq!(storage.head(&k).await.unwrap().unwrap().size, 4);

        // Sobrescribir cambia el tamaño y el etag.
        let second = storage
            .put(&k, Bytes::from_static(b"hola mundo"), PutOptions::new())
            .await
            .unwrap();
        assert_eq!(second.size, 10);
        assert_ne!(second.etag, meta.etag);

        storage.delete(&k).await.unwrap();
        assert!(storage.head(&k).await.unwrap().is_none());
        storage.delete(&k).await.unwrap();
    }

    #[tokio::test]
    async fn rechaza_el_cuerpo_mayor_al_limite() {
        let storage = MemoryStorage::with_max_bytes(4);
        let err = storage
            .put(
                &key("grande"),
                Bytes::from_static(b"12345"),
                PutOptions::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            err.status(),
            rdd_core::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(storage.get(&key("grande")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn dos_tareas_concurrentes_no_corrompen_el_mapa() {
        let storage = std::sync::Arc::new(MemoryStorage::new());
        let mut tasks = Vec::new();
        for index in 0..8 {
            let storage = std::sync::Arc::clone(&storage);
            tasks.push(tokio::spawn(async move {
                let k = key(&format!("files/{index}"));
                storage
                    .put(&k, Bytes::from(vec![index as u8; 32]), PutOptions::new())
                    .await
                    .unwrap();
                storage.get(&k).await.unwrap().unwrap().body.len()
            }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap(), 32);
        }
    }
}
