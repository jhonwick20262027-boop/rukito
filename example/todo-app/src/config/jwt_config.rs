//! Secreto, expiración y bootstrap del admin del demo (§26.3).
//!
//! Solo del entorno (RF-063, RN-030): ni `rdd.toml` ni un valor por defecto
//! usable. Un secreto de ejemplo en el código sería una credencial publicada,
//! y cualquiera podría firmar tokens de admin.

use std::time::Duration;

/// Una hora: suficiente para probar el demo sin dejar el token vivo para siempre.
const DEFAULT_TTL_SECS: u64 = 3600;

/// `JWT_SECRET` obligatoria y no vacía.
///
/// Paniquea si falta, y lo hace desde `main.rs` antes de abrir el socket: arrancar
/// firmando con una cadena vacía sería peor que no arrancar.
pub fn secret() -> String {
    super::env_opt("JWT_SECRET").unwrap_or_else(|| {
        panic!("falta JWT_SECRET: copia .env.example a .env y pon un valor largo (§26.3)")
    })
}

/// `JWT_TTL_SECS`, con respaldo de una hora. Un valor ilegible o cero no puede
/// dejar los tokens expirados al nacer.
pub fn ttl() -> Duration {
    let secs = super::env_opt("JWT_TTL_SECS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTL_SECS)
        .max(1);
    Duration::from_secs(secs)
}

/// El correo que nace `admin` (§26.3). `None` si no está puesta: entonces nadie
/// es admin y `DELETE /tasks/:id` responde 403 para todo el mundo.
pub fn bootstrap_admin() -> Option<String> {
    super::env_opt("ADMIN_EMAIL")
}
