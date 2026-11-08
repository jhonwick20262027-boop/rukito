//! Plantillas del proyecto que genera `rdd new` (§5, RF-002).
//!
//! El árbol de módulo vive en `module_template.rs` porque lo comparten `new` y
//! `g module`. Aquí están la raíz, `src/` fuera de `modules/` y los tests.
//!
//! Los tokens `@nombre@` se sustituyen con `render`; se usan en vez de `format!`
//! para no escapar cada llave del código Rust generado.

use crate::files::GeneratedFile;

/// Datos del proyecto que ya vienen normalizados (RF-142).
pub struct ProjectContext<'a> {
    /// Nombre del paquete Cargo, en `kebab-case`.
    pub package: &'a str,
    /// Nombre de la base de datos, en `snake_case`.
    pub database: &'a str,
    /// Dependencia TOML de `rdd`: versión publicada o ruta local.
    pub rdd_dependency: &'a str,
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
    let vars = [("package", ctx.package), ("database", ctx.database)];
    let mut files = vec![
        GeneratedFile::new("Cargo.toml", cargo_toml(ctx)),
        GeneratedFile::new("rdd.toml", render(RDD_TOML, &vars)),
        GeneratedFile::new("rust-toolchain.toml", RUST_TOOLCHAIN_TOML),
        GeneratedFile::new("rustfmt.toml", RUSTFMT_TOML),
        GeneratedFile::new("clippy.toml", CLIPPY_TOML),
        GeneratedFile::new("Dockerfile", render(DOCKERFILE, &vars)),
        GeneratedFile::new(".env", render(DOTENV, &vars)),
        GeneratedFile::new(".env.example", render(DOTENV_EXAMPLE, &vars)),
        GeneratedFile::new(".gitignore", GITIGNORE),
        GeneratedFile::new("docs/.gitkeep", ""),
        GeneratedFile::new("migrations/.gitkeep", ""),
        GeneratedFile::new("prisma/schema.prisma", schema_prisma(with_example)),
        GeneratedFile::new("prisma/migrations/.gitkeep", ""),
        GeneratedFile::new("tests/acceptance.rs", acceptance_rs(with_example)),
        GeneratedFile::new("tests/e2e/.gitkeep", ""),
        GeneratedFile::new("tests/fixtures/.gitkeep", ""),
        GeneratedFile::new("src/main.rs", main_rs(ctx, with_example)),
        GeneratedFile::new("src/app_module.rs", APP_MODULE_RS),
        GeneratedFile::new("src/app_controller.rs", APP_CONTROLLER_RS),
        GeneratedFile::new("src/app_service.rs", APP_SERVICE_RS),
        GeneratedFile::new("src/config/mod.rs", CONFIG_MOD),
        GeneratedFile::new("src/config/app_config.rs", render(APP_CONFIG_RS, &vars)),
        GeneratedFile::new("src/config/database_config.rs", DATABASE_CONFIG_RS),
        GeneratedFile::new("src/config/jwt_config.rs", JWT_CONFIG_RS),
        GeneratedFile::new("src/config/redis_config.rs", REDIS_CONFIG_RS),
        GeneratedFile::new("src/modules/mod.rs", modules_mod(with_example)),
        GeneratedFile::new("src/generated/mod.rs", GENERATED_MOD),
    ];

    for (parent, doc, children) in FOLDERS {
        files.push(GeneratedFile::new(
            format!("src/{parent}/mod.rs"),
            folder_index(doc, children),
        ));
        for (name, what, api) in *children {
            files.push(GeneratedFile::new(
                format!("src/{parent}/{name}/mod.rs"),
                folder_mod(what, api),
            ));
        }
    }

    files
}

/// Subcarpeta transversal: (nombre, qué va ahí, API de rdd que la cubre).
type Folder = (&'static str, &'static str, &'static str);

/// Carpeta de §5: (nombre, doc de su índice, subcarpetas).
type FolderGroup = (&'static str, &'static str, &'static [Folder]);

/// Índice de una carpeta transversal: declara sus submódulos.
fn folder_index(doc: &str, children: &[Folder]) -> String {
    let mut out = format!(
        "//! {doc}\n//!\n//! Cada subcarpeta documenta qué va ahí y qué API de rdd lo cubre (RF-004).\n\n"
    );
    // rustfmt reordena los `mod` alfabéticamente: se emiten ya ordenados para
    // que el proyecto nazca limpio y `cargo fmt --check` no proponga nada.
    let mut names: Vec<&str> = children.iter().map(|(name, _, _)| *name).collect();
    names.sort_unstable();
    for name in names {
        out.push_str(&format!("pub mod {name};\n"));
    }
    out
}

/// `mod.rs` de una carpeta que todavía no tiene contenido (RF-003, RF-004).
fn folder_mod(what: &str, api: &str) -> String {
    format!("//! {what}\n//!\n//! API de rdd: {api}\n")
}

/// Carpetas transversales de §5: (carpeta, doc del índice, subcarpetas).
///
/// Cada subcarpeta es (nombre, qué va ahí, qué API de rdd lo cubre). Se generan
/// como placeholder compilable porque su soporte llega en fases posteriores.
const FOLDERS: &[FolderGroup] = &[
    (
        "common",
        "Componentes transversales propios del proyecto (§5.1). Lo que es de un solo dominio va en ese módulo, no aquí (RA-004).",
        COMMON,
    ),
    (
        "infrastructure",
        "Servicios técnicos: pool, caché, colas, almacenamiento (§5.1). Los repositories de cada módulo hablan con esta carpeta, nunca al revés.",
        INFRASTRUCTURE,
    ),
    (
        "integrations",
        "Clientes de sistemas externos (§5.1). Un crate externo por integración (`rdd add`, §14.1) registrado como provider; aquí no se decide negocio.",
        INTEGRATIONS,
    ),
    (
        "shared",
        "Tipos, interfaces y contratos que usan varios módulos (§5.1). Si solo los usa un módulo, viven en él (RA-004).",
        SHARED,
    ),
];

const COMMON: &[Folder] = &[
    (
        "decorators",
        "Decoradores propios del proyecto.",
        "los atributos de macro (`#[controller]`, `#[middleware]`, §8); aquí van los que añada el proyecto.",
    ),
    (
        "filters",
        "Filtros de excepción: mapean un error propio a la respuesta HTTP.",
        "`Error` y Problem Details (§10.3); un error de dominio se convierte a `Error` antes de salir del handler.",
    ),
    (
        "guards",
        "Guardas de autorización: cortan la petición antes del handler.",
        "middlewares que devuelven `Error::Unauthorized` o `Error::Forbidden` (§11, RF-082).",
    ),
    (
        "interceptors",
        "Interceptores: envuelven la respuesta para transformar, medir o cachear.",
        "middlewares que llaman `next.run(req)` y post-procesan (§11.2).",
    ),
    (
        "middleware",
        "Middlewares transversales de la app.",
        "`Middleware`, `fn_middleware` y `.middleware(…)` en app, módulo, controller y ruta (§11.3).",
    ),
    (
        "pipes",
        "Transformación y validación de la entrada.",
        "la forma la validan los extractores `Json<T>`, `Query<T>` y `Path<T>` (§10); el negocio, el service.",
    ),
    (
        "constants",
        "Constantes transversales de la app.",
        "Rust plano; las de un dominio van en `modules/<m>/constants/`.",
    ),
    (
        "enums",
        "Enums compartidos por varios módulos.",
        "Rust plano con `serde` cuando viajan en JSON (RF-071).",
    ),
    (
        "interfaces",
        "Traits transversales que no pertenecen a un dominio concreto.",
        "Rust plano; los puertos de un dominio van en `modules/<m>/interfaces/`.",
    ),
    (
        "exceptions",
        "Errores propios de la app.",
        "`Error` (§10.3): `BadRequest`, `Unauthorized`, `Forbidden`, `NotFound`, `Conflict`, `Invalid`, `Internal`.",
    ),
    (
        "utils",
        "Funciones auxiliares sin estado.",
        "Rust plano; si dependen de un dominio, van en ese módulo.",
    ),
];

const INFRASTRUCTURE: &[Folder] = &[
    (
        "database",
        "Pool y acceso técnico a datos.",
        "`App::database(url)`, `Repo<T>` y `SqlxBackend` (§12.1). Llega en la fase 0.2.",
    ),
    (
        "cache",
        "Caché (Redis u otra).",
        "provider del contenedor (§9); la conexión se lee en `config/redis_config.rs`. El núcleo no trae cliente.",
    ),
    (
        "queue",
        "Colas de trabajos en segundo plano.",
        "providers (§9) y `ShutdownSignal` para vaciarlas al apagar (RF-013).",
    ),
    (
        "storage",
        "Almacenamiento de archivos: disco, S3 o equivalente.",
        "provider propio (§9); el cliente concreto va en `integrations/`.",
    ),
    (
        "messaging",
        "Publicación y suscripción interna entre módulos.",
        "providers (§9); los contratos de los mensajes van en `shared/contracts/`.",
    ),
    (
        "external_services",
        "Clientes HTTP de servicios externos.",
        "providers (§9); la configuración va en `src/config/`.",
    ),
];

const INTEGRATIONS: &[Folder] = &[
    (
        "aws",
        "SDK de AWS (S3, SES, SQS…).",
        "crate externo añadido con `rdd add` (§14.1) y registrado como provider (§9).",
    ),
    (
        "telegram",
        "Bot de Telegram.",
        "crate externo (`rdd add`, §14.1) como provider; los handlers entran por un controller.",
    ),
    (
        "email",
        "Envío de correo por SMTP o API.",
        "crate externo (`rdd add`, §14.1) como provider (§9).",
    ),
    (
        "payment_gateways",
        "Pasarelas de pago.",
        "crate externo (`rdd add`, §14.1) como provider; las reglas de negocio van en el service del módulo.",
    ),
];

const SHARED: &[Folder] = &[
    (
        "types",
        "Tipos compartidos: newtypes, identificadores, envolventes.",
        "Rust plano con `serde` cuando viajan en JSON (RF-071).",
    ),
    (
        "interfaces",
        "Traits que implementan varios módulos.",
        "Rust plano; `FromContainer` (§9) cuando el tipo se resuelve por DI.",
    ),
    (
        "contracts",
        "Contratos de los mensajes y eventos que cruzan módulos.",
        "Rust plano con `serde`; el transporte vive en `infrastructure/messaging/`.",
    ),
];

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
    let mut out = String::from(
        "// Fuente de verdad del esquema de datos (§15.2).\n\
         // Prisma se conecta en la fase 0.4; hasta entonces `rdd db generate` no\n\
         // produce nada y `src/generated/` queda vacío.\n\
         \n\
         datasource db {\n  provider = \"postgresql\"\n}\n",
    );
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

fn main_rs(ctx: &ProjectContext<'_>, with_example: bool) -> String {
    let lines = if with_example {
        "        .module(modules::tasks::module())\n"
    } else {
        ""
    };
    render(
        MAIN_RS,
        &[("package", ctx.package), ("module_lines", lines)],
    )
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
provider = "sqlite"            # motor predeterminado
url_env  = "DATABASE_URL"      # nombre de la variable; nunca la URL
max_connections = 10
acquire_timeout_secs = 5
migrations_dir = \"migrations\"

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
DATABASE_URL=sqlite://@package@.db
"#;

const DOTENV_EXAMPLE: &str = r#"# Plantilla versionable de `.env` (RF-063). Nunca poner aquí credenciales reales.

APP_NAME=@package@
HOST=0.0.0.0
PORT=3000
RUST_LOG=info

# SQLite local. La URL va en el entorno, nunca en `rdd.toml` (§15.1).
DATABASE_URL=sqlite://@package@.db

# Reservadas: rdd 0.x no trae autenticación ni caché (ver `src/config/`).
JWT_SECRET=
JWT_EXPIRES_IN_SECS=3600
REDIS_URL=
"#;

const GITIGNORE: &str = r#"# RF-063: `.env` lleva credenciales y no se versiona.
/target
.env
*.log
"#;

const PRISMA_TASK: &str = r#"
model Task {
  id        String   @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  title     String
  done      Boolean  @default(false)
  createdAt DateTime @default(now()) @map("created_at")

  @@map("tasks")
}
"#;

const MAIN_RS: &str = r#"//! Punto de entrada de la app (≈ main.ts, §8.4).
//!
//! `#[rdd::main]` carga `.env`, monta `tracing` con `RUST_LOG` y crea el runtime
//! Tokio; el `main` solo describe la app.

// El scaffold reserva carpetas para fases posteriores (§5, RF-004): parte de su
// contenido todavía no se llama y el proyecto tiene que compilar sin avisos.
#![allow(dead_code)]

use rdd::prelude::*;

mod app_controller;
mod app_module;
mod app_service;
mod common;
mod config;
mod generated;
mod infrastructure;
mod integrations;
mod modules;
mod shared;

#[rdd::main]
async fn main() -> Result<()> {
    let config = config::app_config::load();
    let addr = format!("{}:{}", config.host, config.port);

    // `rdd g module` inserta debajo del marcador; no quitar el comentario (§14.3).
    let app = App::new()
        .config(config)
        // rdd:modules
@module_lines@        .module(app_module::module());

    let app = app.database(config::database_config::url()
        .unwrap_or_else(|| "sqlite://@package@.db".to_owned()));

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
//! `src/modules/<m>/services/` (§5.1).

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
pub mod jwt_config;
pub mod redis_config;

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
//! La URL vive en `DATABASE_URL`, nunca en `rdd.toml` (RF-063). `main.rs` usa
//! SQLite local si no se configura otra URL; `App::database(url)` elige el motor
//! por su esquema (RF-111).

/// `None` cuando no está puesta o está vacía.
pub fn url() -> Option<String> {
    super::env_opt("DATABASE_URL")
}
"#;

const JWT_CONFIG_RS: &str = r#"//! Firma y caducidad de JWT.
//!
//! rdd 0.x no trae paquete de autenticación: esta carpeta reserva el sitio y lee
//! las variables para que no queden dispersas por los módulos (RF-004).

/// Valores que leerá el middleware de autenticación cuando el proyecto lo añada.
pub struct JwtConfig {
    pub secret: String,
    pub expires_in_secs: u64,
}

impl JwtConfig {
    pub fn from_env() -> Self {
        Self {
            secret: super::env_or("JWT_SECRET", ""),
            expires_in_secs: super::env_or("JWT_EXPIRES_IN_SECS", "3600")
                .parse()
                .unwrap_or(3600),
        }
    }
}
"#;

const REDIS_CONFIG_RS: &str = r#"//! Conexión de caché (§5, `infrastructure/cache/`).
//!
//! El núcleo no trae cliente Redis: se conecta como provider (§9) cuando el
//! proyecto lo necesite. Aquí solo se centraliza la lectura de `REDIS_URL`.

/// `None` cuando no está puesta o está vacía.
pub fn url() -> Option<String> {
    super::env_opt("REDIS_URL")
}
"#;

const MODULES_MOD: &str = r#"//! Módulos de dominio de la app (§5).
//!
//! `rdd g module <name>` inserta debajo del marcador; no quitar el comentario
//! (§14.3). Un módulo nunca importa archivos internos de otro (RA-004).

// rdd:mods
"#;

const GENERATED_MOD: &str = r#"//! Código que genera `rdd db generate` (§13.3). NO se edita a mano.
//!
//! En 0.4 Prisma produce aquí los tipos de fila y `Persist` (§12.2). Hasta
//! entonces está vacío a propósito y el comando lo sobrescribe.
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
    let id = json(&resp)["id"].as_u64().expect("el id debe ser numérico");

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
    use crate::modules::tasks::constants::tasks_constants::MAX_TITLE_LEN;

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
