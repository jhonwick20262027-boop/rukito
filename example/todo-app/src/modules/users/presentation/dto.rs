//! Tipos de entrada y salida HTTP de `users` (RF-031, §26.3).
//!
//! El agregado no se deserializa desde la petición. `RegisterDto` no lleva
//! `role`: el privilegio no lo elige el cliente.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::users::domain::{Role, User};

#[derive(Debug, Deserialize)]
pub struct RegisterDto {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginDto {
    pub email: String,
    pub password: String,
}

/// Nunca serializa `password_hash`: la respuesta no puede filtrar el hash.
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            role: user.role,
        }
    }
}

/// Lo que devuelve el login: el token y la cuenta ya sin secretos.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub user: UserResponse,
}
