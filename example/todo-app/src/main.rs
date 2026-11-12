//! Punto de entrada de la app (≈ main.ts, §8.4).
//!
//! `#[rdd::main]` carga `.env`, monta `tracing` con `RUST_LOG` y crea el runtime
//! Tokio; el `main` solo describe la app.

// Parte del scaffold todavía no se llama y el proyecto tiene que compilar sin
// avisos (RF-143); no hay carpetas placeholder que silenciar (RF-004).
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use rdd::AxumPlatform;
use rdd::graphql::{GraphqlHttp, GraphqlHttpConfig, Schema};
use rdd::mcp::{Extensions, McpHttp, McpHttpConfig, McpServer};
use rdd::openapi::{OpenApiHttp, OpenApiHttpConfig};
use rdd::prelude::*;
use rdd::{StaticFiles, StaticFilesConfig};
use rdd_cors::Cors;
use uuid::Uuid;

use crate::middleware::{JwtMiddleware, require_mcp::require_session};
use crate::modules::tasks::presentation::task_tools::{CompleteTask, CreateTask, ListTasks};
use crate::modules::users::{Role, Session, TokenCodec};

mod app_controller;
mod app_module;
mod app_service;
mod config;
mod generated;
mod graphql;
mod middleware;
mod modules;
mod openapi;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);

    // El codec se construye aquí, no dentro del módulo: `users::module` recibe el
    // token codec como parámetro para que las pruebas puedan pasar el suyo sin
    // tocar el entorno (§26.3).
    let tokens = modules::users::tokens();

    // §26.9: el explorer necesita un Bearer. `--explorer-token` lo imprime para
    // una identidad de preview (config/explorer_config.rs) sin registro ni login;
    // la firma es la misma del servidor y caduca con `JWT_TTL_SECS`.
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--explorer-token")
    {
        let (id, email) = config::explorer_config::identity();
        let session = Session {
            id,
            email,
            role: Role::User,
        };
        println!("EXPLORER_TOKEN={}", tokens.issue(&session)?);
    }

    // El mismo hub para el canal y para el controller: los eventos nacen en el
    // REST y salen por `/ws/tasks` (§10.4, §10.5).
    let feed = modules::tasks::TaskFeed::new();

    // `rdd g module` inserta debajo del marcador; no quitar el comentario (§14.3).
    let app = App::new()
        .config(config)
        // El orden importa (RF-080, §26.4): CORS responde el preflight antes de
        // nada, JWT identifica sin rechazar y el rate limit ya tiene identidad.
        .middleware(Cors::from_config().map_err(|err| Error::Internal(err.into()))?)
        .middleware(JwtMiddleware::new(Arc::new(tokens.clone())))
        .middleware(
            RateLimit::new(100, Duration::from_secs(60), rate_key)
                .map_err(|err| Error::Internal(err.into()))?,
        )
        // El canal corre la cadena global en el handshake: el JWT deja la
        // sesión y `TaskFeed::authorize` exige que exista (RF-198, §26.7).
        .provide(feed.clone())
        .socket("/ws/tasks", feed)
        // rdd:modules
        .module(modules::users::module(tokens))
        .module(modules::tasks::module())
        .module(app_module::module());

    // La URL viene solo del entorno (RF-063); el respaldo lo puso
    // `rdd new --database` en `config/database_config.rs` (RF-189).
    let app = app.database(config::database_config::url());

    // `rdd run --memory` deja `RDD_MEMORY=1` para elegir el backend en memoria
    // sin base de datos (§14.1, RF-015).
    let app = if std::env::var("RDD_MEMORY").is_ok() {
        app.in_memory()
    } else {
        app
    };

    // §26.7 y §26.8: un solo registro de tools para los dos transportes. Las
    // tools leen la identidad y el service del `ToolContext` (ADR-25).
    let server = McpServer::new("todo-app", env!("CARGO_PKG_VERSION"))
        .tool(CreateTask)?
        .tool(ListTasks)?
        .tool(CompleteTask)?;

    // §26.7: `--mcp` arranca el mismo contenedor sin abrir puerto. En stdio el
    // proceso es la identidad, así que se siembra la sesión desde `MCP_OWNER_ID`;
    // el filtro por dueño sigue en el caso de uso.
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        let built = app.connect().await?.build()?;
        let owner = std::env::var("MCP_OWNER_ID")
            .ok()
            .and_then(|value| value.parse::<Uuid>().ok())
            .unwrap_or(Uuid::nil());
        let mut ext = Extensions::new();
        ext.insert(Arc::clone(&built.container));
        ext.insert(Session {
            id: owner,
            email: "mcp@local".to_owned(),
            role: Role::User,
        });
        return rdd::mcp::serve_stdio_with(server, ext).await;
    }

    // Los cuatro montajes opcionales comparten router y contenedor; se acumulan
    // para que `--graphql --openapi --static` sirva el explorer, la documentación
    // y la página del demo a la vez sobre el mismo servidor REST.
    let mut platform = AxumPlatform::new();
    let mut mounted = false;

    // §26.8: `--mcp-http` monta `/mcp` en el mismo router y contenedor. La
    // identidad la pone el `JwtMiddleware` global y `require_session` la exige
    // después de la cadena global (RF-231).
    if std::env::args().skip(1).any(|arg| arg == "--mcp-http") {
        let mcp = McpHttp::new(server).config(
            McpHttpConfig::new()
                .allowed_origins(config::mcp_config::allowed_origins())
                .authorize(require_session),
        );
        platform = platform.mcp(mcp);
        mounted = true;
    }

    // §26.9: `--graphql` monta `/graphql` sobre el mismo servidor REST y el mismo
    // contenedor. La cadena global ya corrió en `prepare`, así que `authorize`
    // solo confirma la sesión que dejó `JwtMiddleware` (RF-239).
    if std::env::args().skip(1).any(|arg| arg == "--graphql") {
        let schema = Schema::builder()
            .query(graphql::Query)
            .mutation(graphql::Mutation)
            .subscription(graphql::Subscription)
            .build()?;
        let gql = config::graphql_config::load();
        let graphql = GraphqlHttp::new(schema).config(
            GraphqlHttpConfig::new()
                .path(gql.path)
                .explorer(gql.explorer)
                .public_explorer(gql.public_explorer)
                .max_depth(gql.max_depth)
                .authorize(require_session),
        );
        platform = platform.graphql(graphql);
        mounted = true;
    }

    // §26.10: `--openapi` documenta las mismas rutas en `/api-docs` (más `/docs`
    // y `/swagger`). El documento se compone en `openapi/mod.rs` y la plataforma
    // comprueba que cubra exactamente las rutas montadas (RF-248).
    if std::env::args().skip(1).any(|arg| arg == "--openapi") {
        let docs = config::openapi_config::load();
        let openapi = OpenApiHttp::new(openapi::document()).config(
            OpenApiHttpConfig::new()
                .path(docs.path)
                .docs(docs.docs)
                .public_docs(docs.public_docs)
                .aliases(docs.aliases)
                .strict_routes(docs.strict_routes)
                .authorize(require_session),
        );
        platform = platform.openapi(openapi);
        mounted = true;
    }

    // §26.11: `--static` sirve `client/` en `/site`. La cadena global corre antes
    // que el disco; con `STATIC_PUBLIC=false`, `require_session` exige la sesión
    // que dejó `JwtMiddleware` (RF-259).
    if std::env::args().skip(1).any(|arg| arg == "--static") {
        let site = config::static_config::load();
        let files = StaticFiles::new(&site.dir)
            .and_then(|files| {
                files.config(
                    StaticFilesConfig::new()
                        .mount(site.mount)
                        .spa(site.spa)
                        .public(site.public)
                        .dotfiles(site.dotfiles)
                        .authorize(require_session),
                )
            })
            .map_err(Error::internal)?;
        platform = platform.static_files(files);
        mounted = true;
    }

    if mounted {
        return app.run_on(platform, &addr).await;
    }

    app.run(&addr).await
}

/// Una cuenta no comparte cuota con las demás (RF-109). Los anónimos sí comparten
/// cubo: es lo que frena la fuerza bruta contra `/auth/login`, que no tiene sesión.
fn rate_key(req: &Request) -> String {
    match req.ext::<Session>() {
        Some(session) => format!("user:{}", session.id),
        None => "anonymous".to_owned(),
    }
}
