//! Conexión de caché (§5, `infrastructure/cache/`).
//!
//! El núcleo no trae cliente Redis: se conecta como provider (§9) cuando el
//! proyecto lo necesite. Aquí solo se centraliza la lectura de `REDIS_URL`.

/// `None` cuando no está puesta o está vacía.
pub fn url() -> Option<String> {
    super::env_opt("REDIS_URL")
}
