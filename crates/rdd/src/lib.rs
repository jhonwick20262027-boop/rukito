//! # rdd
//!
//! Fachada del framework (§4.1). Re-exporta el núcleo y las macros, y activa la
//! plataforma por feature; un proyecto de usuario solo declara `rdd` (RA-013).
//!
//! ```no_run
//! use rdd::prelude::*;
//! ```

// El núcleo se re-exporta tal cual: `rdd::http`, `rdd::di`, `rdd::api`,
// `rdd::prelude` y los tipos de raíz (`App`, `Module`, `Error`, …).
pub use rdd_core::*;

pub use rdd_macros::{controller, delete, get, main, middleware, module, patch, post, put};

/// Plataforma por defecto, re-exportada para que el usuario pueda pasarla a
/// `run_on` o configurarla con capas Tower sin declarar `rdd-axum` (RA-013).
#[cfg(feature = "axum")]
pub use rdd_axum::AxumPlatform;

/// Adaptador SQLite predeterminado para almacenamiento persistente local.
#[cfg(feature = "sqlite")]
pub use rdd_sqlite::SqliteAdapter;

/// Lo que expande `#[rdd::main]` (§8.4): instala la plataforma por defecto,
/// carga `.env`, monta `tracing` con `RUST_LOG` (por defecto `info`), crea el
/// runtime Tokio y convierte el `Result` final en código de salida.
///
/// Es `#[doc(hidden)]` porque el usuario no lo llama: lo genera la macro.
#[doc(hidden)]
pub fn run_main<F>(future: F) -> std::process::ExitCode
where
    F: std::future::Future<Output = rdd_core::Result<()>> + Send + 'static,
{
    // RP-012: `App::run` lee la plataforma de este slot. Se instala aquí, y no
    // en un inicializador global, porque la fachada solo puede ejecutar código
    // en el camino que ella misma genera. Es idempotente: la primera escritura
    // gana, así que un `main` propio que ya la instaló no se pisa.
    #[cfg(feature = "axum")]
    rdd_core::http::set_default_platform(|| std::sync::Arc::new(rdd_axum::AxumPlatform::new()));

    #[cfg(feature = "sqlite")]
    rdd_sqlite::register();

    dotenvy::dotenv().ok();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // `try_init`, no `init`: si el binario ya montó su subscriber, no se paniquea.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build();

    match runtime {
        Err(err) => {
            eprintln!("rdd: no se pudo crear el runtime Tokio: {err}");
            std::process::ExitCode::FAILURE
        }
        Ok(runtime) => match runtime.block_on(future) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(err) => {
                // RF-041: el detalle va al log; al cliente solo le afecta si
                // llegó a servir peticiones.
                tracing::error!(error = %err, "la aplicación terminó con error");
                std::process::ExitCode::FAILURE
            }
        },
    }
}
