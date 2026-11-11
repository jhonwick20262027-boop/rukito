//! Modelo y puertos del dominio `users` (§5.1, RA-001).
//!
//! Estos `pub use` son la API de la capa: quien consume importa
//! `…::domain::User`, no `…::domain::user::User`, o el import queda sin usar
//! cuando la re-exportación cambia (RF-007).

pub mod password_hasher;
pub mod role;
pub mod session;
pub mod token_codec;
pub mod user;
pub mod users_repository;

pub use password_hasher::PasswordHasher;
pub use role::Role;
pub use session::Session;
pub use token_codec::TokenCodec;
pub use user::{User, UserError, normalize_email, validate_password};
pub use users_repository::UserRepository;
