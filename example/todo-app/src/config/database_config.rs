//! URL de la base de datos (§15.1, sección `[database]`).
//!
//! La URL vive en `DATABASE_URL`, nunca en `rdd.toml` (RF-063). `App::database(url)`
//! elige el motor por su esquema (RF-111).

/// Respaldo del motor que eligió `rdd new --database` (§14.3, RF-189).
const FALLBACK_URL: &str = "sqlite://todo-app.db";

/// La URL del entorno o, si no está puesta o está vacía, la de respaldo.
pub fn url() -> String {
    super::env_opt("DATABASE_URL").unwrap_or_else(|| FALLBACK_URL.to_owned())
}
