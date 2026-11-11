//! Middlewares transversales de la app (§26.2).
//!
//! No son un módulo de dominio: viven fuera de `modules/` porque los usan varios
//! a la vez. Toman `Session`, `Role` y `TokenCodec` de la **API pública** de
//! `users`, que es lo único que RA-004 permite importar de otro módulo. Al no
//! estar dentro de una capa, RA-001 a RA-003 y RA-007 no les aplican (§17).

pub mod jwt;
pub mod require_auth;
pub mod require_role;
pub mod task_events;

pub use jwt::JwtMiddleware;
pub use require_auth::RequireAuth;
pub use require_role::RequireRole;
pub use task_events::TaskEvents;
