//! Metadatos y opciones que comparten los adaptadores y `MemoryStorage`.

use std::collections::BTreeMap;
use std::time::SystemTime;

use bytes::Bytes;

use crate::key::ObjectKey;

/// Opciones de una escritura (§10.13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PutOptions {
    /// `Content-Type` que se guarda con el objeto.
    pub content_type: Option<String>,
    /// `Cache-Control` para la respuesta del proveedor.
    pub cache_control: Option<String>,
    /// Metadatos de usuario; las claves se envían como `x-amz-meta-*`,
    /// `x-ms-meta-*` o `x-goog-meta-*`.
    pub metadata: BTreeMap<String, String>,
}

impl PutOptions {
    /// Opciones vacías.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fija el tipo de contenido.
    pub fn content_type(mut self, value: impl Into<String>) -> Self {
        self.content_type = Some(value.into());
        self
    }

    /// Fija la política de caché.
    pub fn cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    /// Añade un metadato de usuario.
    pub fn metadata(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(name.into(), value.into());
        self
    }
}

/// Metadata de un objeto, sin su cuerpo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMeta {
    /// Clave del objeto.
    pub key: ObjectKey,
    /// Tamaño en bytes.
    pub size: u64,
    /// `Content-Type` guardado, si el proveedor lo devuelve.
    pub content_type: Option<String>,
    /// ETag opaco: el framework no promete un formato común (RF-271).
    pub etag: Option<String>,
    /// Última modificación, si el proveedor la devuelve.
    pub last_modified: Option<SystemTime>,
}

/// Objeto completo devuelto por `get`.
#[derive(Debug, Clone)]
pub struct StoredObject {
    /// Metadata del objeto.
    pub meta: ObjectMeta,
    /// Cuerpo en memoria (§2.2: sin streaming saliente en esta fase).
    pub body: Bytes,
}
