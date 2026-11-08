//! Transformación entidad ↔ DTO ↔ fila de `tasks` (§5.1).
//!
//! Único sitio donde se decide qué campos viajan en cada dirección. En 0.4
//! entra aquí la fila que genera Prisma (`src/generated/`).

pub mod task_mapper;
