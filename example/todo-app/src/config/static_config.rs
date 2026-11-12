//! Directorio, prefijo y políticas de los archivos estáticos (§10.11, RF-255).
//!
//! Los valores por defecto sirven `client/` en `/site`, sin SPA, sin archivos
//! ocultos y sin exigir JWT: es la página pública del demo. `STATIC_PUBLIC=false`
//! activa `authorize`, que corre después de la cadena global y exige una sesión.

pub struct StaticConfig {
    pub dir: String,
    pub mount: String,
    pub spa: bool,
    pub public: bool,
    pub dotfiles: bool,
}

pub fn load() -> StaticConfig {
    StaticConfig {
        dir: super::env_or("STATIC_DIR", "client"),
        mount: super::env_or("STATIC_MOUNT", "/site"),
        spa: super::env_or("STATIC_SPA", "false") == "true",
        public: super::env_or("STATIC_PUBLIC", "true") == "true",
        dotfiles: super::env_or("STATIC_DOTFILES", "false") == "true",
    }
}
