//! Configuración del montaje de archivos estáticos (§10.11).

use std::sync::Arc;

use rdd_core::http::{Request, normalize_path};

use crate::DEFAULT_INDEX;
use crate::DEFAULT_MOUNT;

/// Guardián ejecutado después de los globales (RF-259).
pub type AuthorizeFn = Arc<dyn Fn(&Request) -> rdd_core::Result<()> + Send + Sync>;

/// Política de un montaje estático.
///
/// Los valores por defecto sirven la raíz del directorio en `/`, sin SPA, sin
/// archivos ocultos y con `/assets` como prefijo inmutable.
pub struct StaticFilesConfig {
    mount: String,
    index: String,
    spa: bool,
    public: bool,
    dotfiles: bool,
    immutable: Vec<String>,
    authorize: Option<AuthorizeFn>,
}

impl Default for StaticFilesConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticFilesConfig {
    /// `/` con `index.html`, sin SPA y con `/assets` inmutable.
    pub fn new() -> Self {
        Self {
            mount: DEFAULT_MOUNT.to_owned(),
            index: DEFAULT_INDEX.to_owned(),
            spa: false,
            public: false,
            dotfiles: false,
            immutable: vec!["/assets".to_owned()],
            authorize: None,
        }
    }

    /// Prefijo donde se montan los archivos (`/` por defecto).
    pub fn mount(mut self, mount: impl Into<String>) -> Self {
        self.mount = mount.into();
        self
    }

    /// Archivo que se sirve al pedir un directorio (`index.html` por defecto).
    pub fn index(mut self, index: impl Into<String>) -> Self {
        self.index = index.into();
        self
    }

    /// `true` devuelve el índice ante una ruta desconocida sin extensión.
    pub fn spa(mut self, enabled: bool) -> Self {
        self.spa = enabled;
        self
    }

    /// `true` omite `authorize`; los middlewares globales corren igual.
    pub fn public(mut self, enabled: bool) -> Self {
        self.public = enabled;
        self
    }

    /// `true` permite servir archivos y carpetas que empiezan por punto.
    pub fn dotfiles(mut self, enabled: bool) -> Self {
        self.dotfiles = enabled;
        self
    }

    /// Prefijo con `Cache-Control: immutable`; puede llamarse varias veces.
    pub fn immutable(mut self, prefix: impl Into<String>) -> Self {
        self.immutable.push(prefix.into());
        self
    }

    /// Se ejecuta después de los globales; `Err` se responde como Problem Details.
    pub fn authorize<F>(mut self, f: F) -> Self
    where
        F: Fn(&Request) -> rdd_core::Result<()> + Send + Sync + 'static,
    {
        self.authorize = Some(Arc::new(f));
        self
    }

    /// Prefijo del montaje, tal como se configuró.
    pub fn mount_path(&self) -> &str {
        &self.mount
    }

    /// Archivo índice.
    pub fn index_file(&self) -> &str {
        &self.index
    }

    /// Si el fallback de SPA está activo.
    pub fn spa_enabled(&self) -> bool {
        self.spa
    }

    /// Si `authorize` se omite.
    pub fn is_public(&self) -> bool {
        self.public
    }

    /// Si se sirven archivos ocultos.
    pub fn dotfiles_enabled(&self) -> bool {
        self.dotfiles
    }

    /// Prefijos con caché inmutable.
    pub fn immutable_prefixes(&self) -> &[String] {
        &self.immutable
    }

    /// Guardián configurado, si lo hay.
    pub fn authorize_fn(&self) -> Option<&AuthorizeFn> {
        self.authorize.as_ref()
    }

    /// Comprueba la configuración antes de abrir el socket (RF-255).
    pub fn validate(&self) -> anyhow::Result<()> {
        let mount = normalize_path(&self.mount);
        let literal = self.mount.starts_with('/')
            && self.mount == mount
            && !self
                .mount
                .contains([':', '*', '{', '}', '?', '#', '\\', ' ', '\t', '\n']);
        if !literal {
            anyhow::bail!(
                "prefijo estático inválido `{}`: se espera una ruta literal como `/site` o `/`",
                self.mount
            );
        }

        let index_ok = !self.index.is_empty()
            && self.index != "."
            && self.index != ".."
            && !self
                .index
                .contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
            && !self.index.chars().any(char::is_control);
        if !index_ok {
            anyhow::bail!(
                "archivo índice inválido `{}`: se espera un nombre como `index.html`",
                self.index
            );
        }

        for prefix in &self.immutable {
            let normalized = normalize_path(prefix);
            let literal = prefix.starts_with('/')
                && prefix == &normalized
                && !prefix.contains([':', '*', '{', '}', '?', '#', '\\', ' ', '\t', '\n']);
            if !literal {
                anyhow::bail!(
                    "prefijo inmutable inválido `{prefix}`: se espera una ruta literal como `/assets`"
                );
            }
        }
        Ok(())
    }
}
