//! Ruta, UI y políticas de `/api-docs` (§10.10, RF-250).
//!
//! Los valores por defecto sirven la documentación en `/api-docs`, con alias
//! `/docs` y `/swagger`, sin exigir JWT para leer el documento (las operaciones
//! que el cliente lance desde la UI sí lo exigen) y con rutas estrictas: toda
//! ruta montada tiene que estar documentada. En producción conviene
//! `OPENAPI_PUBLIC_DOCS=false`.

pub struct OpenApiConfig {
    pub path: String,
    pub docs: bool,
    pub public_docs: bool,
    pub aliases: Vec<String>,
    pub strict_routes: bool,
}

pub fn load() -> OpenApiConfig {
    OpenApiConfig {
        path: super::env_or("OPENAPI_PATH", "/api-docs"),
        docs: super::env_or("OPENAPI_DOCS", "true") == "true",
        public_docs: super::env_or("OPENAPI_PUBLIC_DOCS", "true") == "true",
        aliases: super::env_or("OPENAPI_ALIASES", "/docs,/swagger")
            .split(',')
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .map(str::to_owned)
            .collect(),
        strict_routes: super::env_or("OPENAPI_STRICT", "true") == "true",
    }
}
