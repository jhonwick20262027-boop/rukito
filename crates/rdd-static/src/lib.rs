//! Archivos estáticos para rdd (§10.11).
//!
//! El crate es neutral: resuelve una ruta bajo una raíz con defensas contra
//! traversal, decide MIME, ETag, caché, índice y fallback de SPA, y devuelve una
//! respuesta con los tipos de `rdd_core::http`. El montaje —leer el cuerpo con
//! límite, ejecutar `HttpApp::prepare` y volcar la respuesta al router— lo hace
//! `rdd-axum` con la feature `static-files` (RP-033).
//!
//! ```no_run
//! use rdd_static::{StaticFiles, StaticFilesConfig};
//!
//! # fn main() -> anyhow::Result<()> {
//! let site = StaticFiles::new("client")?.config(
//!     StaticFilesConfig::new()
//!         .mount("/site")
//!         .spa(true)
//!         .immutable("/assets"),
//! )?;
//! # let _ = site;
//! # Ok(())
//! # }
//! ```

mod config;
mod files;
mod path;

pub use config::{AuthorizeFn, StaticFilesConfig};
pub use files::StaticFiles;

/// Prefijo por defecto del montaje.
pub const DEFAULT_MOUNT: &str = "/";

/// Archivo índice por defecto.
pub const DEFAULT_INDEX: &str = "index.html";
