//! Configuración por dominio, centralizada (≈ configuration.ts, §5).
//!
//! `.env` lo carga `#[rdd::main]` (RF-063). La lectura de `rdd.toml` (§15) llega
//! con el cargador de configuración; hasta entonces cada archivo de esta carpeta
//! es el único sitio donde se decide de dónde sale un valor.

pub mod app_config;
pub mod database_config;
pub mod jwt_config;
pub mod mcp_config;

/// Variable de entorno con valor por defecto; la usan los archivos de al lado.
pub(crate) fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_owned())
}

/// Variable de entorno opcional: cadena vacía cuenta como no puesta.
pub(crate) fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}
