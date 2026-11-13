//! Driver y límite de `--upload` (§10.13, §26.13, RF-277).
//!
//! `UPLOAD_DRIVER` elige el adaptador en tiempo de ejecución (`memory` por
//! defecto) y `UPLOAD_MAX_BYTES` fija a la vez el límite del objeto y el del
//! cuerpo de la petición. Con una nube, sus credenciales salen de las variables
//! de §10.13; un secreto ausente falla el arranque nombrando la variable.

use std::sync::Arc;

use rdd::upload::{AzureBlobStorage, GcsStorage, MemoryStorage, ObjectStorage, S3Storage};
use rdd::{Error, Result};

/// 8 MiB: el mismo defecto que documenta §26.13.
pub const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024;

pub fn max_bytes() -> usize {
    super::env_or("UPLOAD_MAX_BYTES", "8388608")
        .parse()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES)
}

pub fn storage() -> Result<Arc<dyn ObjectStorage>> {
    let driver = super::env_or("UPLOAD_DRIVER", "memory");
    // Un fallo de configuración es del operador, no del cliente (RF-041, RF-275).
    let config_err = |err: rdd::upload::UploadConfigError| Error::internal(err.into());
    match driver.as_str() {
        "memory" => Ok(Arc::new(MemoryStorage::with_max_bytes(max_bytes()))),
        "aws" => S3Storage::from_env()
            .map(|storage| Arc::new(storage) as Arc<dyn ObjectStorage>)
            .map_err(config_err),
        "azure" => AzureBlobStorage::from_env()
            .map(|storage| Arc::new(storage) as Arc<dyn ObjectStorage>)
            .map_err(config_err),
        "gcs" => GcsStorage::from_env()
            .map(|storage| Arc::new(storage) as Arc<dyn ObjectStorage>)
            .map_err(config_err),
        other => Err(Error::internal(anyhow::anyhow!(
            "UPLOAD_DRIVER desconocido: {other}"
        ))),
    }
}
