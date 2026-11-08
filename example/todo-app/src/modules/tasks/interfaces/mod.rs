//! Traits del dominio `tasks` (§5.1, RA-001).
//!
//! El service depende del puerto, nunca de la implementación; así cambiar de
//! backend no toca las reglas de negocio.

pub mod task_interface;
