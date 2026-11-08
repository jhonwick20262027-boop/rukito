//! Nombre, host y puerto de la app (§15.1, sección `[app]`).

/// Los valores por defecto son los de §15.1; `main.rs` los pasa a `App::config`.
pub fn load() -> rdd::AppConfig {
    rdd::AppConfig {
        name: super::env_or("APP_NAME", "todo-app"),
        host: super::env_or("HOST", "0.0.0.0"),
        port: super::env_or("PORT", "3000").parse().unwrap_or(3000),
    }
}
