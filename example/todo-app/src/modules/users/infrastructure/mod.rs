//! Adaptadores técnicos del módulo `users` (§5.1, RA-007).
//!
//! Implementan los puertos definidos por el dominio. Es la única capa que nombra
//! `argon2` y `jsonwebtoken`: cambiarlos no toca el caso de uso.

pub mod argon2_hasher;
pub mod jwt_tokens;
pub mod users_repository;
