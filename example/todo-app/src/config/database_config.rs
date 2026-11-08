//! URL de la base de datos (§15.1, sección `[database]`).
//!
//! La URL vive en `DATABASE_URL`, nunca en `rdd.toml` (RF-063). `main.rs` usa
//! SQLite local si no se configura otra URL; `App::database(url)` elige el motor
//! por su esquema (RF-111).

/// `None` cuando no está puesta o está vacía.
pub fn url() -> Option<String> {
    super::env_opt("DATABASE_URL")
}
