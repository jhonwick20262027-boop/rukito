//! Agregados y value objects de `tasks` (§5.1, RA-001).
//!
//! En 0.2 `#[aggregate]` añade `Persist` (§12.2) y las reglas de dominio; en 0.1
//! es una estructura plana con `serde` (RF-006).

pub mod task_entity;
