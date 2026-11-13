//! Casos de uso de `files` (§5.1, RA-002, §26.13).
//!
//! La clave `files/{owner_id}/{id}` se construye aquí con la sesión y un `Uuid`
//! del servidor: el cliente nunca propone una clave, así que no puede leer ni
//! borrar lo de otro. El puerto de almacenamiento no sabe quién pregunta.

use rdd::prelude::*;
use rdd::upload::{ObjectKey, ObjectStorage, PutOptions};
use uuid::Uuid;

use crate::modules::files::domain::{FileMeta, FileName};

pub struct FilesUseCases<S: ObjectStorage> {
    storage: S,
}

impl<S: ObjectStorage> FilesUseCases<S> {
    pub fn with_storage(storage: S) -> Self {
        Self { storage }
    }

    /// Guarda el cuerpo y devuelve la metadata del objeto creado.
    pub async fn upload_of(
        &self,
        owner: Uuid,
        name: FileName,
        content_type: String,
        body: Bytes,
    ) -> Result<FileMeta> {
        let id = Uuid::new_v4();
        let key = key_of(owner, id);
        let options = PutOptions::new()
            .content_type(content_type)
            // El nombre original viaja como metadato de usuario; el demo no lo
            // repite en la respuesta porque `head` no lo devuelve (RF-271).
            .metadata("original-name", name.as_str());
        let meta = self.storage.put(&key, body, options).await?;
        Ok(FileMeta::new(id, meta.size, meta.content_type))
    }

    /// Metadata del objeto propio; 404 si no existe o es de otro.
    pub async fn meta_of(&self, owner: Uuid, id: Uuid) -> Result<FileMeta> {
        let key = key_of(owner, id);
        let meta = self
            .storage
            .head(&key)
            .await?
            .ok_or_else(|| Error::not_found(format!("no existe el archivo {id}")))?;
        Ok(FileMeta::new(id, meta.size, meta.content_type))
    }

    /// Borra el objeto propio; un id ajeno o inexistente es 404.
    pub async fn remove_of(&self, owner: Uuid, id: Uuid) -> Result<()> {
        let key = key_of(owner, id);
        if self.storage.head(&key).await?.is_none() {
            return Err(Error::not_found(format!("no existe el archivo {id}")));
        }
        self.storage.delete(&key).await
    }
}

/// La clave solo puede fallar si el puerto cambiara su política (RF-270): con
/// dos UUID no hay segmentos vacíos ni traversal.
fn key_of(owner: Uuid, id: Uuid) -> ObjectKey {
    ObjectKey::parse(format!("files/{owner}/{id}"))
        .expect("files/{owner}/{id} cumple la política de claves")
}
