//! Adaptadores HTTP de `tasks` (§5.1, RA-003).
//!
//! Un controller extrae parámetros, llama al service y devuelve; no decide
//! negocio. Lo cubre `#[controller]` con los verbos `#[get]`, `#[post]`,
//! `#[put]`, `#[patch]` y `#[delete]` (§8).

pub mod tasks_controller;
