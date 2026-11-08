//! Conversión del error de sqlx al del núcleo (RF-042).
//!
//! Vive aquí, no en `rdd-core`: el núcleo no conoce `sqlx::Error` (RA-014).

use rdd_core::error::Error;

/// Fila ausente → `NotFound`, unicidad → `Conflict`, resto → `Internal`.
pub fn map(err: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db) = &err
        && db.is_unique_violation()
    {
        return Error::conflict(format!("violación de unicidad: {}", db.message()));
    }
    match err {
        sqlx::Error::RowNotFound => Error::not_found("la consulta no devolvió ninguna fila"),
        other => Error::internal(anyhow::Error::new(other)),
    }
}
