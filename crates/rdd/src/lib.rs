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

/// JSON, tal cual, para que lo generado por las macros no obligue al proyecto a
/// declarar `serde_json` (RN-012).
pub use serde_json;

/// Macros del esquema GraphQL (§10.9, RF-240).
#[cfg(feature = "graphql")]
pub use rdd_macros::{GraphqlInput, GraphqlObject, graphql_object};

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

/// Servidor MCP por stdio (§10.6, §12.6); lo activa la feature `mcp`.
///
/// No hay nada que registrar: la app resuelve sus services y construye las tools
/// en `presentation/`, igual que un controller (RA-019, ADR-22).
#[cfg(feature = "mcp")]
pub mod mcp {
    pub use rdd_mcp::{
        CallToolResult, Content, Extensions, McpHttp, McpHttpConfig, McpServer, Tool, ToolContext,
        serve_io, serve_io_with, serve_stdio, serve_stdio_with,
    };
}

/// Re-export de las piezas que documenta §12.6, por comodidad.
#[cfg(feature = "mcp")]
pub use rdd_mcp::{McpServer, Tool, serve_stdio, serve_stdio_with};

/// Transporte MCP sobre Streamable HTTP (§10.8); lo activa la feature
/// `mcp-http`. El montaje lo aporta `AxumPlatform::mcp` (RA-020).
#[cfg(feature = "mcp-http")]
pub use rdd_mcp::{McpHttp, McpHttpConfig, McpHttpResponse};

/// GraphQL como entrada opcional sobre el mismo `HttpApp` (§10.9, RF-243).
#[cfg(feature = "graphql")]
pub mod graphql {
    pub use rdd_graphql::{
        ArgDef, FieldDef, GraphqlContext, GraphqlHttp, GraphqlHttpConfig, GraphqlHttpResponse,
        GraphqlInput, GraphqlObjectType, GraphqlOutput, GraphqlRequest, GraphqlRoot, GraphqlStream,
        GraphqlValue, InputFieldDef, InputObjectDef, ObjectTypeDef, ResolveFn, RootField, RootKind,
        Schema, SchemaBuilder, SelectedField, SelectionSet, SseStream, SubscribeFn, TypeRef,
        TypeRegistry, argument, resolved, serialize_typename,
    };

    pub use rdd_macros::{GraphqlInput, GraphqlObject, graphql_object};
}

/// GraphQL sin prefijo de módulo, para el código de `main` y los resolutores.
#[cfg(feature = "graphql")]
pub use rdd_graphql::{
    GraphqlContext, GraphqlHttp, GraphqlHttpConfig, GraphqlRequest, GraphqlStream, Schema,
    SchemaBuilder,
};

#[cfg(all(feature = "graphql", not(feature = "axum")))]
compile_error!(
    "la feature `graphql` necesita la plataforma `axum`: activa `rdd` con \
     features = [\"axum\", \"graphql\"] (RP-012)"
);

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
