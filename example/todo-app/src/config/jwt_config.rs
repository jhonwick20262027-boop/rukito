//! Firma y caducidad de JWT.
//!
//! rdd 0.x no trae paquete de autenticación: esta carpeta reserva el sitio y lee
//! las variables para que no queden dispersas por los módulos (RF-004).

/// Valores que leerá el middleware de autenticación cuando el proyecto lo añada.
pub struct JwtConfig {
    pub secret: String,
    pub expires_in_secs: u64,
}

impl JwtConfig {
    pub fn from_env() -> Self {
        Self {
            secret: super::env_or("JWT_SECRET", ""),
            expires_in_secs: super::env_or("JWT_EXPIRES_IN_SECS", "3600")
                .parse()
                .unwrap_or(3600),
        }
    }
}
