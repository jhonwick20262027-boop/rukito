//! DTO de salida HTTP para `files` (§26.13).
//!
//! El cuerpo binario no vuelve en esta fase (RF-277, RF-278): la respuesta es
//! solo la metadata del objeto.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::files::domain::FileMeta;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileResponse {
    pub id: Uuid,
    pub size: u64,
    pub content_type: Option<String>,
}

impl From<FileMeta> for FileResponse {
    fn from(meta: FileMeta) -> Self {
        Self {
            id: meta.id,
            size: meta.size,
            content_type: meta.content_type,
        }
    }
}
