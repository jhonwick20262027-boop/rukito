//! Acceso a datos de `tasks` (§5.1).
//!
//! Implementa el puerto de `interfaces/` y habla con `infrastructure/`. En 0.2
//! envuelve `Repo<T>` (§12.1) sobre memoria o PostgreSQL; en 0.1 el almacén es
//! un `Vec` en memoria porque todavía no hay persistencia (RF-006).

pub mod tasks_repository;
