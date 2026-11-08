//! Contratos de entrada de `tasks` (§5.1).
//!
//! Un DTO por caso de uso. La validación de forma (tipos, campos requeridos) la
//! hacen `Json<T>` y `Query<T>` (§10); la de negocio vive en `services/`.

pub mod create_task_dto;
pub mod query_task_dto;
pub mod update_task_dto;
