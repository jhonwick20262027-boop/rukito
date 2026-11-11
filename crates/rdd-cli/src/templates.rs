//! Plantillas del proyecto que genera `rdd new` (§5, RF-002).
//!
//! El árbol de módulo vive en `module_template.rs` porque lo comparten `new` y
//! `g module`. Aquí están la raíz, `src/` fuera de `modules/` y los tests.
//!
//! Los tokens `@nombre@` se sustituyen con `render`; se usan en vez de `format!`
//! para no escapar cada llave del código Rust generado.

use crate::files::GeneratedFile;

/// Motor predeterminado del proyecto (§14.1, RF-189).
///
/// Solo los que tienen feature en la fachada (§12.6); la lista crece con cada
/// crate de motor que pase la suite de conformidad (RA-016).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Database {
    #[default]
    Sqlite,
    Postgres,
}

impl Database {
    /// Feature de la fachada que trae el adaptador. Es también el `provider` de
    /// `rdd.toml` (§15.1), que no usa el token de Prisma.
    pub fn feature(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }

    /// Token del `datasource` de Prisma (§15.2): PostgreSQL se llama
    /// `postgresql` ahí, no `postgres`.
    pub fn prisma_provider(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgresql",
        }
    }

    /// URL de respaldo de `main.rs`, `.env` y `.env.example` (§14.3). La de
    /// PostgreSQL lleva un marcador por contraseña: no hay servidor que adivinar
    /// y RF-063 prohíbe inventar credenciales.
    pub fn default_url(self, package: &str, database: &str) -> String {
        match self {
            Self::Sqlite => format!("sqlite://{package}.db"),
            Self::Postgres => format!("postgres://postgres:CAMBIAR@localhost:5432/{database}"),
        }
    }
}

/// Datos del proyecto que ya vienen normalizados (RF-142).
pub struct ProjectContext<'a> {
    /// Nombre del paquete Cargo, en `kebab-case`.
    pub package: &'a str,
    /// Nombre de la base de datos, en `snake_case`.
    pub database: &'a str,
    /// Dependencia TOML de `rdd`: versión publicada o ruta local.
    pub rdd_dependency: &'a str,
    /// Motor predeterminado (RF-189).
    pub engine: Database,
}

/// Sustituye `@clave@` por su valor.
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (key, value) in vars {
        out = out.replace(&format!("@{key}@"), value);
    }
    out
}

/// Todos los archivos del proyecto, salvo el módulo de ejemplo.
pub fn project_files(ctx: &ProjectContext<'_>, with_example: bool) -> Vec<GeneratedFile> {
    let url = ctx.engine.default_url(ctx.package, ctx.database);
    let hint = match ctx.engine {
        Database::Sqlite => "# SQLite local: no necesita servidor.",
        Database::Postgres => {
            "# Sustituye `CAMBIAR` por la contraseña real: PostgreSQL necesita un servidor."
        }
    };
    let vars = [
        ("package", ctx.package),
        ("provider", ctx.engine.feature()),
        ("prisma_provider", ctx.engine.prisma_provider()),
        ("database_url", url.as_str()),
        ("database_hint", hint),
    ];
    let files = vec![
        GeneratedFile::new("Cargo.toml", cargo_toml(ctx)),
        GeneratedFile::new("rdd.toml", render(RDD_TOML, &vars)),
        GeneratedFile::new("rust-toolchain.toml", RUST_TOOLCHAIN_TOML),
        GeneratedFile::new("rustfmt.toml", RUSTFMT_TOML),
        GeneratedFile::new("clippy.toml", CLIPPY_TOML),
        GeneratedFile::new("Dockerfile", render(DOCKERFILE, &vars)),
        GeneratedFile::new(".env", render(DOTENV, &vars)),
        GeneratedFile::new(".env.example", render(DOTENV_EXAMPLE, &vars)),
        GeneratedFile::new(".gitignore", gitignore(ctx.engine)),
        // RF-003/RF-192: con módulo de ejemplo la carpeta ya tiene contenido real.
        // `.gitkeep` solo hace falta sin él, porque git no trackea directorios vacíos.
        if with_example {
            GeneratedFile::new("migrations/0001_tasks.sql", TASKS_MIGRATION)
        } else {
            GeneratedFile::new("migrations/.gitkeep", "")
        },
        GeneratedFile::new(
            "prisma/schema.prisma",
            render(&schema_prisma(with_example), &vars),
        ),
        GeneratedFile::new("tests/acceptance.rs", acceptance_rs(with_example)),
        GeneratedFile::new("src/main.rs", main_rs(with_example)),
        GeneratedFile::new("src/app_module.rs", APP_MODULE_RS),
        GeneratedFile::new("src/app_controller.rs", APP_CONTROLLER_RS),
        GeneratedFile::new("src/app_service.rs", APP_SERVICE_RS),
        GeneratedFile::new("src/config/mod.rs", CONFIG_MOD),
        GeneratedFile::new("src/config/app_config.rs", render(APP_CONFIG_RS, &vars)),
        GeneratedFile::new(
            "src/config/database_config.rs",
            render(DATABASE_CONFIG_RS, &vars),
        ),
        GeneratedFile::new("src/modules/mod.rs", modules_mod(with_example)),
        GeneratedFile::new("src/generated/mod.rs", GENERATED_MOD),
    ];

    files
}

fn cargo_toml(ctx: &ProjectContext<'_>) -> String {
    format!(
        "# ≈ package.json + tsconfig.json (§5). El MSRV lo fija `rust-version`;\n\
         # `rust-toolchain.toml` solo declara el perfil.\n\
         [package]\n\
         name = \"{package}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         rust-version = \"1.94\"\n\
         description = \"Aplicación rdd generada con `rdd new`.\"\n\
         publish = false\n\
         \n\
         [dependencies]\n\
         # RA-013: el proyecto depende de la fachada, nunca de `rdd-core`.\n\
         rdd = {dependency}\n\
         serde = {{ version = \"1\", features = [\"derive\"] }}\n\
         serde_json = \"1\"\n\
         # El id del agregado es un UUID (§7.2); `serde` lo serializa como texto.\n\
         uuid = {{ version = \"1\", features = [\"v4\", \"serde\"] }}\n\
         \n\
         [dev-dependencies]\n\
         bytes = \"1\"\n\
         http = \"1\"\n\
         tokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\n",
        package = ctx.package,
        dependency = ctx.rdd_dependency,
    )
}

fn schema_prisma(with_example: bool) -> String {
    let mut out = String::from(SCHEMA_PRISMA);
    if with_example {
        out.push_str(PRISMA_TASK);
    }
    out
}

fn modules_mod(with_example: bool) -> String {
    let mut out = String::from(MODULES_MOD);
    if with_example {
        out.push_str("pub mod tasks;\n");
    }
    out
}

fn main_rs(with_example: bool) -> String {
    let lines = if with_example {
        "        .module(modules::tasks::module())\n"
    } else {
        ""
    };
    render(MAIN_RS, &[("module_lines", lines)])
}

fn acceptance_rs(with_example: bool) -> String {
    let lines = if with_example {
        "        .module(modules::tasks::module())\n"
    } else {
        ""
    };
    let tasks = if with_example { TASKS_TESTS } else { "" };
    render(
        ACCEPTANCE_RS,
        &[("module_lines", lines), ("tasks_tests", tasks)],
    )
}

const RDD_TOML: &str = r#"# Configuración del proyecto (§15.1).
# RF-150: las secciones desconocidas se ignoran; RF-151: cada paquete externo
# lee solo la suya.

[app]
name = "@package@"
host = "0.0.0.0"
port = 3000
body_limit_bytes = 2_097_152

[database]
# El motor lo decide el esquema de la URL (RF-111); `provider` es orientativo y
# lo lee `rdd doctor`. Salió de `rdd new --database` (RF-189).
provider = "@provider@"
url_env  = "DATABASE_URL"      # nombre de la variable; nunca la URL (RF-063)
max_connections = 10
acquire_timeout_secs = 5
migrations_dir = "migrations"  # lo lee `rdd migrate` (§14.1)

[prisma]
schema        = "prisma/schema.prisma"
generated_dir = "src/generated"
# cli_version se fija al iniciar la fase 0.4 (§15.1).

[cors]                          # leído solo por rdd-cors (fase 0.3)
allow_origins     = ["http://localhost:5173"]
allow_methods     = ["GET", "POST", "PUT", "PATCH", "DELETE"]
allow_headers     = ["content-type", "authorization"]
allow_credentials = false
max_age_secs      = 3600

[logger]                        # leído solo por rdd-logger (fase 0.3)
format = "pretty"               # "pretty" | "json"
"#;

const RUST_TOOLCHAIN_TOML: &str = r#"# ≈ tsconfig.json en lo relativo al toolchain (§5).
#
# No se fija `channel`: un canal pinned obligaría a rustup a descargar otro
# toolchain y `rdd new` dejaría de compilar sin red (RF-143, RF-007). El mínimo
# soportado lo declara `rust-version` en Cargo.toml.

[toolchain]
profile = "minimal"
components = ["rustfmt", "clippy"]
"#;

const RUSTFMT_TOML: &str = r#"# ≈ .prettierrc (§5). Estilo por defecto de rustfmt; solo se fija el edition
# para que coincida con Cargo.toml.

edition = "2024"
"#;

const CLIPPY_TOML: &str = r#"# ≈ .eslintrc.json (§5). Se exige con `cargo clippy -- -D warnings`.

msrv = "1.94"
"#;

const DOCKERFILE: &str = r#"# Imagen de la app. Requiere `rdd` publicado en crates.io; mientras tanto,
# cambia la dependencia de Cargo.toml a una ruta o a un git.

FROM rust:1 AS build
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/@package@ /usr/local/bin/app
ENV HOST=0.0.0.0 PORT=3000
EXPOSE 3000
CMD ["app"]
"#;

const DOTENV: &str = r#"# Valores locales. NO versionado (RF-063): `.gitignore` lo excluye.
# Copia `.env.example` para ver todas las variables que lee `src/config/`.

APP_NAME=@package@
HOST=0.0.0.0
PORT=3000
RUST_LOG=info

@database_hint@
DATABASE_URL=@database_url@
"#;

const DOTENV_EXAMPLE: &str = r#"# Plantilla versionable de `.env` (RF-063). Nunca poner aquí credenciales reales.

APP_NAME=@package@
HOST=0.0.0.0
PORT=3000
RUST_LOG=info

# La URL va en el entorno, nunca en `rdd.toml` (§15.1). El motor lo decide su
# esquema (RF-111); este valor salió de `rdd new --database` (RF-189).
@database_hint@
DATABASE_URL=@database_url@

"#;

fn gitignore(engine: Database) -> String {
    let mut out = String::from(
        "# RF-063: `.env` lleva credenciales y no se versiona.\n/target\n.env\n*.log\n",
    );
    if engine == Database::Sqlite {
        out.push_str(
            "\n# La base local la crea `rdd migrate` y lleva datos de usuario (RF-063).\n\
             *.db\n*.db-shm\n*.db-wal\n",
        );
    }
    out
}

const SCHEMA_PRISMA: &str = r#"// Fuente de verdad del esquema de datos (§15.2).
// Prisma se conecta en la fase 0.4; hasta entonces `rdd db generate` no
// produce nada y `src/generated/` queda vacío.

datasource db {
  provider = "@prisma_provider@"
}
"#;

// Sin `createdAt`: `Value` no tiene variante temporal (V15) y el adaptador la
// rechazaría al leer (RF-187), rompiendo el round-trip de `Persist` (§12.2).
const PRISMA_TASK: &str = r#"
model Task {
  id    String  @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  title String
  done  Boolean @default(false)

  @@map("tasks")
}
"#;

const MAIN_RS: &str = r#"//! Punto de entrada de la app (≈ main.ts, §8.4).
//!
//! `#[rdd::main]` carga `.env`, monta `tracing` con `RUST_LOG` y crea el runtime
//! Tokio; el `main` solo describe la app.

// Parte del scaffold todavía no se llama y el proyecto tiene que compilar sin
// avisos (RF-143); no hay carpetas placeholder que silenciar (RF-004).
#![allow(dead_code)]

use rdd::prelude::*;

mod app_controller;
mod app_module;
mod app_service;
mod config;
mod generated;
mod modules;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);

    // `rdd g module` inserta debajo del marcador; no quitar el comentario (§14.3).
    let app = App::new()
        .config(config)
        // rdd:modules
@module_lines@        .module(app_module::module());

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

    app.run(&addr).await
}
"#;

const APP_MODULE_RS: &str = r#"//! Módulo raíz de la app (≈ app.module.ts, §6.2).
//!
//! Aquí van los componentes transversales. Los módulos de dominio se montan en
//! `main.rs`, debajo de `// rdd:modules` (§14.3).

use rdd::module;

use crate::app_controller::AppController;

/// El módulo raíz listo para montar.
pub fn module() -> rdd::Module {
    module! {
        providers: [],
        controllers: [AppController],
        middlewares: [],
    }
}
"#;

const APP_CONTROLLER_RS: &str = r#"//! Controller raíz (≈ app.controller.ts, §8.2).
//!
//! `GET /` responde JSON (RF-070). `/health` lo añade el núcleo, no este
//! controller, y se puede quitar con `App::without_health` (RF-016).

use rdd::controller;
use rdd::prelude::*;

use crate::app_service::AppService;

pub struct AppController;

#[controller("/")]
impl AppController {
    /// Bienvenida: confirma que la app está montada y respondiendo.
    #[get("/")]
    async fn index() -> Result<String> {
        Ok(AppService::hello())
    }
}
"#;

const APP_SERVICE_RS: &str = r#"//! Servicio de la raíz (≈ app.service.ts).
//!
//! Sin reglas de dominio: los casos de uso reales viven en
//! `src/modules/<m>/application/` (§5.1).

pub struct AppService;

impl AppService {
    /// Mensaje que devuelve `AppController`.
    pub fn hello() -> String {
        format!("{} en línea", env!("CARGO_PKG_NAME"))
    }
}
"#;

const CONFIG_MOD: &str = r#"//! Configuración por dominio, centralizada (≈ configuration.ts, §5).
//!
//! `.env` lo carga `#[rdd::main]` (RF-063). La lectura de `rdd.toml` (§15) llega
//! con el cargador de configuración; hasta entonces cada archivo de esta carpeta
//! es el único sitio donde se decide de dónde sale un valor.

pub mod app_config;
pub mod database_config;

/// Variable de entorno con valor por defecto; la usan los archivos de al lado.
pub(crate) fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_owned())
}

/// Variable de entorno opcional: cadena vacía cuenta como no puesta.
pub(crate) fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}
"#;

const APP_CONFIG_RS: &str = r#"//! Nombre, host y puerto de la app (§15.1, sección `[app]`).

/// Los valores por defecto son los de §15.1; `main.rs` los pasa a `App::config`.
pub fn load() -> rdd::AppConfig {
    rdd::AppConfig {
        name: super::env_or("APP_NAME", "@package@"),
        host: super::env_or("HOST", "0.0.0.0"),
        port: super::env_or("PORT", "3000").parse().unwrap_or(3000),
    }
}
"#;

const DATABASE_CONFIG_RS: &str = r#"//! URL de la base de datos (§15.1, sección `[database]`).
//!
//! La URL vive en `DATABASE_URL`, nunca en `rdd.toml` (RF-063). `App::database(url)`
//! elige el motor por su esquema (RF-111).

/// Respaldo del motor que eligió `rdd new --database` (§14.3, RF-189).
const FALLBACK_URL: &str = "@database_url@";

/// La URL del entorno o, si no está puesta o está vacía, la de respaldo.
pub fn url() -> String {
    super::env_opt("DATABASE_URL").unwrap_or_else(|| FALLBACK_URL.to_owned())
}
"#;

const MODULES_MOD: &str = r#"//! Módulos de dominio de la app (§5).
//!
//! `rdd g module <name>` inserta debajo del marcador; no quitar el comentario
//! (§14.3). Un módulo nunca importa archivos internos de otro (RA-004).

// rdd:mods
"#;

/// Migración inicial del módulo de ejemplo (RF-192). La aplica `rdd migrate`.
///
/// El DDL sirve para SQLite y PostgreSQL. `UUID` y `BOOLEAN` son obligatorios
/// aunque SQLite los guarde como texto y 0/1: `rdd-sqlite` decodifica por el
/// nombre de tipo declarado, así que con `TEXT`/`INTEGER` el `from_row` del
/// agregado recibiría `Value::Text`/`Value::I64` en vez de `Uuid`/`Bool`.
const TASKS_MIGRATION: &str = r#"-- Migración inicial del módulo de ejemplo (RF-192).
-- La aplica `rdd migrate` (RF-188); el servidor no migra al arrancar (RF-117).
-- Los tipos declarados son los que esperan los adaptadores: SQLite guarda el id
-- como texto y el booleano como 0/1, pero decodifica por el nombre del tipo.
CREATE TABLE IF NOT EXISTS tasks (
    id    UUID PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    done  BOOLEAN NOT NULL DEFAULT FALSE
);
"#;

/// `src/generated/mod.rs` (RF-004).
///
/// El código de verdad llega en 0.4 con `rdd db generate`. Hasta entonces el
/// archivo lleva un snippet **documental** en un bloque `ignore`: cumple «ningún
/// archivo en blanco» sin generar código que dependa de una API sin implementar
/// (RF-006), y el bloque no se compila ni rompe `cargo test` (RF-007).
const GENERATED_MOD: &str = r#"//! Código que genera `rdd db generate` (§13.3). NO se edita a mano.
//!
//! En 0.4 Prisma produce aquí los tipos de fila y sus `impl Persist` (§12.2) a
//! partir de `prisma/schema.prisma`. Este snippet es documental: muestra la forma
//! exacta de lo que va a llegar, y está marcado `ignore` porque el generador
//! todavía no existe (RF-004, RF-006).
//!
//! Mientras tanto, el agregado del módulo de ejemplo implementa `Aggregate` y
//! `Persist` a mano en `modules/tasks/domain/task.rs` (§14.2). Cuando el
//! generador entre, ese impl se sustituye por el de aquí y el dominio se queda
//! solo con las invariantes.
//!
//! ```rust,ignore
//! // Lo emitirá `rdd db generate` (RF-133: determinista, mismo esquema → mismo archivo).
//! #[derive(Debug, Clone, serde::Serialize)]
//! pub struct TaskRow {
//!     pub id: uuid::Uuid,
//!     pub title: String,
//!     pub done: bool,
//! }
//!
//! impl rdd::Aggregate for TaskRow {
//!     type Id = uuid::Uuid;
//!
//!     fn id(&self) -> &Self::Id {
//!         &self.id
//!     }
//! }
//!
//! impl rdd::Persist for TaskRow {
//!     const TABLE: &'static str = "tasks";
//!     const COLUMNS: &'static [&'static str] = &["id", "title", "done"];
//!
//!     fn to_values(&self) -> Vec<rdd::Value> {
//!         vec![
//!             rdd::Value::Uuid(self.id),
//!             rdd::Value::from(self.title.clone()),
//!             rdd::Value::Bool(self.done),
//!         ]
//!     }
//!
//!     fn from_row(row: &rdd::Row) -> rdd::Result<Self> {
//!         // Por nombre y no por posición: el orden de `Row` no es contractual (§12.1).
//!         Ok(Self {
//!             id: row.value("id")?.as_uuid()?,
//!             title: row.value("title")?.as_text()?.to_owned(),
//!             done: row.value("done")?.as_bool()?,
//!         })
//!     }
//! }
//! ```
"#;

const ACCEPTANCE_RS: &str = r#"//! Aceptación de 0.1 (§21, RF-007): el proyecto compila, responde en `/health`
//! y devuelve 404 y 405 en formato Problem Details (§10.3).
//!
//! Un test de integración no puede importar el binario, así que el árbol de
//! `src/` se incluye por ruta: `crate::modules::…` y `crate::app_*` resuelven
//! igual que en `main.rs` y no hay que duplicar el cableado.

#![allow(dead_code)]

#[path = "../src/app_controller.rs"]
mod app_controller;
#[path = "../src/app_module.rs"]
mod app_module;
#[path = "../src/app_service.rs"]
mod app_service;
#[path = "../src/modules/mod.rs"]
mod modules;

use rdd::http::{Method, header, into_response};
use rdd::prelude::*;
use rdd::{controller, module};

fn app() -> HttpApp {
    App::new()
        .module(app_module::module())
@module_lines@        .build()
        .expect("la app debe construirse")
        .http
}

fn request(method: Method, uri: &str, body: &str) -> Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes::Bytes::copy_from_slice(body.as_bytes()))
        .expect("petición válida")
}

async fn send(http: &HttpApp, method: Method, uri: &str, body: &str) -> Response {
    http.dispatch(request(method, uri, body)).await
}

fn json(resp: &Response) -> serde_json::Value {
    serde_json::from_slice(resp.body().as_ref()).expect("el cuerpo debe ser JSON")
}

/// §10.3: toda respuesta de error es `application/problem+json`.
fn problem(resp: &Response) -> serde_json::Value {
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/problem+json"),
    );
    json(resp)
}

#[tokio::test]
async fn health_responde_ok() {
    let http = app();
    let resp = send(&http, Method::GET, "/health", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["status"], "ok");
}

#[tokio::test]
async fn la_raiz_responde_json() {
    let http = app();
    let resp = send(&http, Method::GET, "/", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
}

#[tokio::test]
async fn ruta_inexistente_es_404_en_problem_details() {
    let http = app();
    let resp = send(&http, Method::GET, "/no-existe", "").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = problem(&resp);
    assert_eq!(body["status"], 404);
    assert_eq!(body["instance"], "/no-existe");
}

#[tokio::test]
async fn metodo_no_permitido_es_405_con_allow() {
    let http = app();
    let resp = send(&http, Method::POST, "/", "").await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        resp.headers()
            .get(header::ALLOW)
            .is_some_and(|value| value.to_str().unwrap().contains("GET")),
        "falta la cabecera Allow (RF-072)"
    );
    assert_eq!(problem(&resp)["status"], 405);
}

#[test]
fn todos_los_errores_salen_en_problem_details() {
    let cases = [
        (Error::bad_request("mal formado"), 400),
        (Error::Unauthorized, 401),
        (Error::Forbidden, 403),
        (Error::not_found("no existe"), 404),
        (Error::conflict("ya existe"), 409),
        (Error::invalid("no procesable"), 422),
    ];
    for (err, status) in cases {
        let resp = into_response(StatusCode::OK, Err::<(), _>(err));
        assert_eq!(resp.status().as_u16(), status);
        let body = problem(&resp);
        assert_eq!(body["type"], "about:blank");
        assert_eq!(body["status"], status);
        assert!(body["title"].is_string());
        assert!(body["detail"].is_string());
    }
}

/// Controller de prueba: verifica que un panic no tumba la app (RF-073).
struct BoomController;

#[controller("/boom")]
impl BoomController {
    #[get("/")]
    async fn boom() -> Result<()> {
        panic!("fallo provocado para verificar RF-073");
    }
}

#[tokio::test]
async fn un_panico_es_500_y_no_filtra_el_detalle() {
    let http = App::new()
        .module(module! { controllers: [BoomController], middlewares: [] })
        .build()
        .expect("la app debe construirse")
        .http;
    let resp = send(&http, Method::GET, "/boom", "").await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = problem(&resp);
    assert_eq!(body["status"], 500);
    // RF-041: el detalle interno va al log, no al cliente.
    assert!(
        !body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("provocado"),
        "el 500 no debe exponer el mensaje del panic"
    );
}
@tasks_tests@"#;

const TASKS_TESTS: &str = r##"
#[tokio::test]
async fn crud_del_modulo_de_ejemplo() {
    let http = app();

    let resp = send(&http, Method::POST, "/tasks", r#"{"title":"pan"}"#).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id = json(&resp)["id"]
        .as_str()
        .expect("el id debe ser un UUID en texto")
        .to_owned();
    uuid::Uuid::parse_str(&id).expect("el id debe ser un UUID válido (§7.2)");

    let resp = send(&http, Method::GET, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["title"], "pan");

    let resp = send(&http, Method::GET, "/tasks?q=pan", "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp).as_array().unwrap().len(), 1);

    let done = r#"{"done":true}"#;
    let resp = send(&http, Method::PUT, &format!("/tasks/{id}"), done).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json(&resp)["done"], true);

    let resp = send(&http, Method::DELETE, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.body().is_empty(), "204 no lleva cuerpo (RF-053)");

    let resp = send(&http, Method::GET, &format!("/tasks/{id}"), "").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(problem(&resp)["status"], 404);
}

#[tokio::test]
async fn validacion_del_modulo_de_ejemplo() {
    use crate::modules::tasks::domain::task::MAX_TITLE_LEN;

    let http = app();

    let vacio = r#"{"title":"  "}"#;
    let resp = send(&http, Method::POST, "/tasks", vacio).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["instance"], "/tasks");

    let largo = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_TITLE_LEN + 1));
    let resp = send(&http, Method::POST, "/tasks", &largo).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem(&resp)["status"], 422);

    // RF-055: un parámetro de ruta que no convierte es 400.
    let resp = send(&http, Method::GET, "/tasks/abc", "").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(problem(&resp)["status"], 400);
}
"##;
