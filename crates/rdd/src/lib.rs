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

/// Adaptador PostgreSQL; lo activa la feature `postgres` (§12.6).
#[cfg(feature = "postgres")]
pub use rdd_postgres::PostgresAdapter;

/// Hub de salas y presencia del tiempo real (§10.5); lo activa la feature
/// `socket`. El puerto `.socket(...)` viene del núcleo y no depende de esto.
#[cfg(feature = "socket")]
pub use rdd_socket::{Hub, Lagged, Payload, Subscription};

/// Puebla el registro de adaptadores del núcleo (RF-191).
///
/// Los seis motores se declaran siempre, aunque su feature esté apagada: es lo
/// que permite que el error de RF-111 nombre el crate y la feature que faltan en
/// vez de decir «esquema no soportado». Solo se instala la factoría de los
/// compilados.
fn register_adapters() {
    use rdd_core::db::registry::declare;

    declare(&["sqlite"], "rdd-sqlite", "sqlite");
    declare(&["postgres", "postgresql"], "rdd-postgres", "postgres");
    declare(&["mysql", "mariadb"], "rdd-mysql", "mysql");
    declare(&["mongodb"], "rdd-mongodb", "mongodb");
    declare(&["redis"], "rdd-redis", "redis");
    declare(&["sqlserver"], "rdd-mssql", "mssql");

    #[cfg(feature = "sqlite")]
    rdd_sqlite::register();

    #[cfg(feature = "postgres")]
    rdd_postgres::register();
}

/// Lo que expande `#[rdd::main]` (§8.4): instala la plataforma por defecto,
/// registra los adaptadores de base de datos, carga `.env`, monta `tracing` con
/// `RUST_LOG` (por defecto `info`), crea el runtime Tokio y convierte el `Result`
/// final en código de salida.
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

    // RF-119: `App::connect()` resuelve la URL contra este registro, así que
    // tiene que estar poblado antes de que el `main` del usuario llegue a
    // `.database(url)`.
    register_adapters();

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

#[cfg(test)]
mod tests {
    use rdd_core::db::{declared, factory_for};

    use super::register_adapters;

    /// RF-191: los seis esquemas de RF-111 quedan declarados aunque su crate no
    /// esté compilado, que es lo que da el mensaje con nombre y feature.
    #[test]
    fn declara_los_seis_motores() {
        register_adapters();
        let schemes = declared();
        for expected in [
            "sqlite",
            "postgres",
            "postgresql",
            "mysql",
            "mariadb",
            "mongodb",
            "redis",
            "sqlserver",
        ] {
            assert!(
                schemes.iter().any(|scheme| scheme == expected),
                "falta `{expected}` en {schemes:?}"
            );
        }
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_se_instala_solo() {
        register_adapters();
        assert!(factory_for("sqlite://app.db").is_ok());
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_se_instala_con_su_feature() {
        register_adapters();
        assert!(factory_for("postgres://localhost/db").is_ok());
        assert!(factory_for("postgresql://localhost/db").is_ok());
    }

    #[cfg(not(feature = "postgres"))]
    #[test]
    fn postgres_sin_feature_nombra_el_crate_que_falta() {
        register_adapters();
        let err = factory_for("postgres://localhost/db")
            .unwrap_err()
            .to_string();
        assert!(err.contains("rdd-postgres"), "{err}");
        assert!(err.contains("postgres"), "{err}");
    }
}
